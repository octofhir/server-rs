//! CQL evaluation service

use crate::config::CqlConfig;
use crate::data_provider::FhirServerDataProvider;
use crate::error::{CqlError, CqlResult};
use crate::library_cache::LibraryCache;
use crate::terminology_provider::CqlTerminologyProvider;
use indexmap::IndexMap;
use octofhir_cql::parse;
use octofhir_cql_eval::CqlEngine;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// A single validation finding from [`CqlService::validate_source`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationIssue {
    pub severity: String,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

pub struct CqlService {
    engine: Arc<CqlEngine>,
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
            engine: Arc::new(CqlEngine::new()),
            data_provider,
            terminology_provider,
            library_cache,
            storage,
            config,
        }
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
        // Parse CQL source to AST
        let ast_library = parse(cql_source).map_err(|e| CqlError::ParseError(format!("{}", e)))?;

        // Convert AST to ELM
        let elm_library = {
            use octofhir_cql::elm::AstToElmConverter;
            let mut converter = AstToElmConverter::new();
            converter.convert_library(&ast_library)
        };

        // Build evaluation context using builder pattern
        let mut ctx_builder = octofhir_cql_eval::EvaluationContextBuilder::new()
            .data_provider(self.data_provider.clone())
            .terminology_provider(self.terminology_provider.clone());

        // Set context
        if let Some(ct) = context_type
            && let Some(cv) = context_value
            && let Some(cql_val) = super::data_provider::json_to_cql_value(&cv)
        {
            ctx_builder = ctx_builder.context(ct, cql_val);
        }

        // Set parameters
        for (name, value) in parameters {
            if let Some(cql_val) = super::data_provider::json_to_cql_value(&value) {
                ctx_builder = ctx_builder.parameter(name, cql_val);
            }
        }

        let mut ctx = ctx_builder.build();

        // Evaluate the ELM library — returns every define's value
        let results = self
            .engine
            .evaluate_library(&elm_library, &mut ctx)
            .map_err(|e| CqlError::EvaluationError(format!("{:?}", e)))?;

        // Convert every definition to JSON, preserving source (define) order
        let mut json_results = IndexMap::new();
        for (name, value) in results {
            let json = super::data_provider::cql_value_to_json(&value).unwrap_or(Value::Null);
            json_results.insert(name, json);
        }

        Ok(json_results)
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

        // Parse library CQL source to AST
        let cql_source = &_library.cql_source;
        let ast_library = parse(cql_source).map_err(|e| CqlError::ParseError(format!("{}", e)))?;

        // Convert AST to ELM
        let elm_library = {
            use octofhir_cql::elm::AstToElmConverter;
            let mut converter = AstToElmConverter::new();
            converter.convert_library(&ast_library)
        };

        // Build evaluation context using builder pattern
        let mut ctx_builder = octofhir_cql_eval::EvaluationContextBuilder::new()
            .data_provider(self.data_provider.clone())
            .terminology_provider(self.terminology_provider.clone());

        if let Some(ct) = context_type
            && let Some(cv) = context_value
            && let Some(cql_val) = super::data_provider::json_to_cql_value(&cv)
        {
            ctx_builder = ctx_builder.context(ct, cql_val);
        }

        for (name, value) in parameters {
            if let Some(cql_val) = super::data_provider::json_to_cql_value(&value) {
                ctx_builder = ctx_builder.parameter(name, cql_val);
            }
        }

        let mut ctx = ctx_builder.build();

        // Evaluate all definitions
        let results = self
            .engine
            .evaluate_library(&elm_library, &mut ctx)
            .map_err(|e| CqlError::EvaluationError(format!("{:?}", e)))?;

        // Convert to JSON
        let mut json_results = HashMap::new();
        for (name, value) in results {
            if let Some(json) = super::data_provider::cql_value_to_json(&value) {
                json_results.insert(name, json);
            }
        }

        Ok(json_results)
    }

    /// Parse-only validation — no ELM conversion, no evaluation, no data
    /// provider. Fast enough to run on every keystroke (debounced) to surface
    /// syntax errors early. Returns an empty list when the source is valid.
    ///
    /// Note: cql-rs spans are currently placeholders, so `line`/`column` are not
    /// yet populated — only the message is reliable.
    pub fn validate_source(&self, cql_source: &str) -> Vec<ValidationIssue> {
        if cql_source.trim().is_empty() {
            return Vec::new();
        }
        match parse(cql_source) {
            Ok(_) => Vec::new(),
            Err(e) => vec![ValidationIssue {
                severity: "error".to_string(),
                message: format!("{}", e),
                line: None,
                column: None,
            }],
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
