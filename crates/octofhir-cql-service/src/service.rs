//! CQL evaluation service

use crate::config::CqlConfig;
use crate::data_provider::FhirServerDataProvider;
use crate::error::{CqlError, CqlResult};
use crate::library_cache::LibraryCache;
use crate::terminology_provider::CqlTerminologyProvider;
use indexmap::IndexMap;
use octofhir_cql::{CompiledLibrary as CqlLibrary, parse};
use octofhir_cql_model::fhir::json::fhir_to_cql;
use octofhir_cql_types::{CqlDateTime, CqlInterval, CqlType, CqlValue};
use octofhir_storage::SearchParams;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Included libraries one evaluation may load, transitively.
const MAX_INCLUDED_LIBRARIES: usize = 64;

/// Name of the library the evaluator implements itself.
const FHIR_HELPERS: &str = "FHIRHelpers";

/// Source of an included library, by name and requested version.
type IncludedSources = HashMap<(String, Option<String>), String>;

/// A single validation finding from [`CqlService::validate_source`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationIssue {
    pub severity: String,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Clone)]
pub struct CqlService {
    data_provider: Arc<FhirServerDataProvider>,
    terminology_provider: Arc<CqlTerminologyProvider>,
    library_cache: Arc<LibraryCache>,
    storage: octofhir_storage::DynStorage,
    config: CqlConfig,
}

impl CqlService {
    pub fn new(
        data_provider: Arc<FhirServerDataProvider>,
        terminology_provider: Arc<CqlTerminologyProvider>,
        library_cache: Arc<LibraryCache>,
        storage: octofhir_storage::DynStorage,
        config: CqlConfig,
    ) -> Self {
        Self {
            data_provider,
            terminology_provider,
            library_cache,
            storage,
            config,
        }
    }

    /// Clone the service with an evaluation-local data provider. Never mutate
    /// the shared service or place a caller's identity in a global cache.
    pub fn with_data_provider(&self, provider: Arc<FhirServerDataProvider>) -> Self {
        let mut service = self.clone();
        service.data_provider = provider;
        service
    }

    pub async fn evaluate_expression(
        &self,
        expression: &str,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<Value> {
        tracing::info!(expression = expression, "Evaluating CQL expression");

        if expression.is_empty() {
            return Err(CqlError::InvalidParameter(
                "Expression cannot be empty".to_string(),
            ));
        }

        let result = tokio::time::timeout(
            Duration::from_millis(self.config.evaluation_timeout_ms),
            self.evaluate_expression_internal(expression, context_type, context_value, parameters),
        )
        .await
        .map_err(|_| {
            CqlError::Timeout(format!(
                "Expression evaluation timed out after {}ms",
                self.config.evaluation_timeout_ms
            ))
        })??;

        Ok(result)
    }

    async fn evaluate_expression_internal(
        &self,
        expression: &str,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<Value> {
        // Wrap expression in a minimal ad-hoc library with a single `Result` define.
        let library_cql = format!(
            r#"library Adhoc version '1.0.0'

define Result:
  {}
"#,
            expression
        );

        let results = self
            .eval_source(&library_cql, context_type, context_value, parameters)
            .await?;

        Ok(results.get("Result").cloned().unwrap_or(Value::Null))
    }

    /// Parse, compile and evaluate raw CQL library source, returning every
    /// `define` statement's value keyed by name.
    ///
    /// Shared by ad-hoc expression evaluation, stored-library evaluation and
    /// inline library-source evaluation.
    async fn eval_source(
        &self,
        cql_source: &str,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<IndexMap<String, Value>> {
        let timeout = Duration::from_millis(self.config.evaluation_timeout_ms);
        let deadline = std::time::Instant::now() + timeout;
        let data_provider = Arc::new(self.data_provider.for_evaluation(deadline));
        let includes = self.included_sources(cql_source).await?;
        let terminology_provider = self.terminology_provider.clone();
        let cql_source = cql_source.to_owned();
        let context_type = context_type.map(str::to_owned);
        let runtime = tokio::runtime::Handle::current();
        let job = tokio::task::spawn_blocking(move || -> CqlResult<IndexMap<String, Value>> {
            // The evaluator has synchronous callbacks. Enter the runtime on a
            // blocking thread so callbacks can await SQL/FHIRPath safely even
            // when the caller runs on Tokio's single-thread runtime.
            let _entered = runtime.enter();
            // Parse and translate the library with the libraries it includes.
            let load = |name: &str, version: Option<&str>| {
                includes
                    .get(&(name.to_owned(), version.map(str::to_owned)))
                    .cloned()
            };
            let library = CqlLibrary::compile_with(&cql_source, &load)
                .map_err(|e| CqlError::ParseError(e.to_string()))?;

            // Build evaluation context using builder pattern
            let mut ctx_builder = octofhir_cql_eval::EvaluationContextBuilder::new()
                .data_provider(data_provider.clone())
                .terminology_provider(terminology_provider);

            // Set context
            if let Some(ct) = context_type
                && let Some(cv) = context_value
            {
                // A FHIR resource (the Patient): typed as FHIRHelpers would.
                let cql_val = fhir_to_cql(&cv);
                ctx_builder = ctx_builder.context(ct, cql_val);
            }

            // Set parameters
            for (name, value) in parameters {
                if let Some(cql_val) = super::data_provider::json_to_cql_value(&value) {
                    ctx_builder = ctx_builder.parameter(name, cql_val);
                }
            }

            let mut ctx = ctx_builder.build();
            library.register(&mut ctx);

            // Evaluate the library — returns every define's value
            let result = library.evaluate(&mut ctx);
            if let Some(error) = data_provider.take_error() {
                return Err(error);
            }
            let results =
                result.map_err(|error| CqlError::EvaluationError(format!("{error:?}")))?;

            // Convert every definition to JSON, preserving source (define) order
            let mut json_results = IndexMap::new();
            for (name, value) in results {
                let json = super::data_provider::cql_value_to_json(&value).unwrap_or(Value::Null);
                json_results.insert(name, json);
            }

            Ok(json_results)
        });
        tokio::time::timeout(timeout, job)
            .await
            .map_err(|_| CqlError::Timeout("CQL evaluation timed out".into()))?
            .map_err(|error| CqlError::Other(format!("CQL evaluation task failed: {error}")))?
    }

    /// The sources of the libraries `source` includes, transitively, from the
    /// stored `Library` resources (by `name`, and `version` when the include
    /// names one). `FHIRHelpers` is built into the evaluator and never loaded.
    async fn included_sources(&self, source: &str) -> CqlResult<IncludedSources> {
        let mut sources = IncludedSources::new();
        let mut pending = vec![source.to_owned()];
        while let Some(text) = pending.pop() {
            // A parse error is reported when the library compiles.
            let Ok(ast) = parse(&text) else { continue };
            for include in &ast.includes {
                let name = include.inner.library.name.name.clone();
                let version = include.inner.version.as_ref().map(|v| v.version.clone());
                let key = (name.clone(), version.clone());
                if name == FHIR_HELPERS || sources.contains_key(&key) {
                    continue;
                }
                if sources.len() >= MAX_INCLUDED_LIBRARIES {
                    return Err(CqlError::InvalidParameter(format!(
                        "A library may include at most {MAX_INCLUDED_LIBRARIES} libraries"
                    )));
                }
                let library = self.stored_library(&name, version.as_deref()).await?;
                let included = LibraryCache::extract_cql_content(&library)?;
                pending.push(included.clone());
                sources.insert(key, included);
            }
        }
        Ok(sources)
    }

    /// The stored `Library` named `name` (and `version`), the latest one when
    /// several versions match.
    async fn stored_library(&self, name: &str, version: Option<&str>) -> CqlResult<Value> {
        let mut params = SearchParams::new().with_param("name", name).with_count(50);
        if let Some(version) = version {
            params = params.with_param("version", version);
        }
        let found = self.storage.search("Library", &params).await?;
        found
            .entries
            .into_iter()
            .map(|entry| entry.resource)
            .filter(|library| library["name"] == name)
            .max_by(|a, b| {
                a["version"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["version"].as_str().unwrap_or_default())
            })
            .ok_or_else(|| {
                CqlError::LibraryNotFound(format!(
                    "Included library {name}{} is not stored on this server",
                    version
                        .map(|v| format!(" version '{v}'"))
                        .unwrap_or_default()
                ))
            })
    }

    /// Evaluate an inline CQL library (full source with one or more `define`
    /// statements) and return every definition's value keyed by name.
    ///
    /// Unlike [`evaluate_library`](Self::evaluate_library), the source is supplied
    /// directly rather than loaded from a stored `Library` resource — powering the
    /// interactive CQL console.
    pub async fn evaluate_library_source(
        &self,
        cql_source: &str,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<IndexMap<String, Value>> {
        if cql_source.trim().is_empty() {
            return Err(CqlError::InvalidParameter(
                "Library source cannot be empty".to_string(),
            ));
        }

        tracing::info!("Evaluating inline CQL library source");

        let results = tokio::time::timeout(
            Duration::from_millis(self.config.evaluation_timeout_ms),
            self.eval_source(cql_source, context_type, context_value, parameters),
        )
        .await
        .map_err(|_| {
            CqlError::Timeout(format!(
                "Library evaluation timed out after {}ms",
                self.config.evaluation_timeout_ms
            ))
        })??;

        Ok(results)
    }

    /// The CQL text of a stored Library.
    pub async fn library_source(&self, url: &str, version: Option<&str>) -> CqlResult<String> {
        let library = self
            .library_cache
            .get_or_compile(url, version.unwrap_or("latest"), &self.storage)
            .await?;
        Ok(library.cql_source.clone())
    }

    /// Evaluate one library for many subjects, compiling it once: the shape of a measure
    /// population. `period` is the `Measurement Period` parameter as two instants.
    pub async fn evaluate_subjects(
        &self,
        cql_source: &str,
        context_type: &str,
        subjects: Vec<Value>,
        period: (&str, &str),
    ) -> CqlResult<Vec<IndexMap<String, Value>>> {
        let boundary = |value: &str| {
            CqlDateTime::parse(value)
                .map(|instant| Box::new(CqlValue::DateTime(instant)))
                .ok_or_else(|| CqlError::InvalidParameter(format!("Invalid instant {value}")))
        };
        let measurement_period = CqlValue::Interval(CqlInterval {
            point_type: CqlType::DateTime,
            low: Some(boundary(period.0)?),
            low_closed: true,
            high: Some(boundary(period.1)?),
            high_closed: true,
        });
        let includes = self.included_sources(cql_source).await?;
        let timeout = Duration::from_millis(self.config.evaluation_timeout_ms);
        let provider = self.data_provider.clone();
        let terminology_provider = self.terminology_provider.clone();
        let cql_source = cql_source.to_owned();
        let context_type = context_type.to_owned();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || -> CqlResult<Vec<IndexMap<String, Value>>> {
            let _entered = runtime.enter();
            let load = |name: &str, version: Option<&str>| {
                includes
                    .get(&(name.to_owned(), version.map(str::to_owned)))
                    .cloned()
            };
            let library = CqlLibrary::compile_with(&cql_source, &load)
                .map_err(|e| CqlError::ParseError(e.to_string()))?;
            let mut results = Vec::with_capacity(subjects.len());
            for subject in subjects {
                let data_provider =
                    Arc::new(provider.for_evaluation(std::time::Instant::now() + timeout));
                let mut ctx = octofhir_cql_eval::EvaluationContextBuilder::new()
                    .data_provider(data_provider.clone())
                    .terminology_provider(terminology_provider.clone())
                    .context(context_type.clone(), fhir_to_cql(&subject))
                    .parameter("Measurement Period".to_owned(), measurement_period.clone())
                    .build();
                library.register(&mut ctx);
                let result = library.evaluate(&mut ctx);
                if let Some(error) = data_provider.take_error() {
                    return Err(error);
                }
                let defines =
                    result.map_err(|error| CqlError::EvaluationError(format!("{error:?}")))?;
                results.push(
                    defines
                        .into_iter()
                        .map(|(name, value)| {
                            let json = super::data_provider::cql_value_to_json(&value)
                                .unwrap_or(Value::Null);
                            (name, json)
                        })
                        .collect(),
                );
            }
            Ok(results)
        })
        .await
        .map_err(|error| CqlError::EvaluationError(error.to_string()))?
    }

    pub async fn evaluate_library(
        &self,
        library_url: &str,
        version: Option<&str>,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<HashMap<String, Value>> {
        let version = version.unwrap_or("latest");

        tracing::info!(
            library_url = library_url,
            version = version,
            "Evaluating CQL library"
        );

        let _library = self
            .library_cache
            .get_or_compile(library_url, version, &self.storage)
            .await?;

        Ok(self
            .eval_source(
                &_library.cql_source,
                context_type,
                context_value,
                parameters,
            )
            .await?
            .into_iter()
            .collect())
    }

    /// Parse-only validation — no ELM conversion, no evaluation, no data
    /// provider. Fast enough to run on every keystroke (debounced) to surface
    /// syntax errors early. Returns an empty list when the source is valid.
    ///
    /// Every syntax error is reported with its 1-based line and column.
    pub fn validate_source(&self, cql_source: &str) -> Vec<ValidationIssue> {
        if cql_source.trim().is_empty() {
            return Vec::new();
        }
        match parse(cql_source) {
            Ok(_) => Vec::new(),
            Err(error) => {
                let location = error.location();
                vec![ValidationIssue {
                    severity: "error".to_string(),
                    message: error.to_string(),
                    line: location.map(|location| location.line),
                    column: location.map(|location| location.column),
                }]
            }
        }
    }

    pub fn cache_stats(&self) -> crate::library_cache::CacheStats {
        self.library_cache.stats()
    }

    pub fn clear_cache(&self) {
        self.library_cache.clear();
    }
}

#[cfg(test)]
mod tests;
