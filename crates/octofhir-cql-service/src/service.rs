//! CQL evaluation service

use crate::config::CqlConfig;
use crate::data_provider::FhirServerDataProvider;
use crate::error::{CqlError, CqlResult};
use crate::library_cache::LibraryCache;
use crate::terminology_provider::CqlTerminologyProvider;
use octofhir_cql::parse;
use octofhir_cql_eval::CqlEngine;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

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
            return Err(CqlError::InvalidParameter("Expression cannot be empty".to_string()));
        }

        let result = tokio::time::timeout(
            Duration::from_millis(self.config.evaluation_timeout_ms),
            self.evaluate_expression_internal(expression, context_type, context_value, parameters),
        )
        .await
        .map_err(|_| CqlError::Timeout(format!("Expression evaluation timed out after {}ms", self.config.evaluation_timeout_ms)))??;

        Ok(result)
    }

    async fn evaluate_expression_internal(
        &self,
        expression: &str,
        context_type: Option<&str>,
        context_value: Option<Value>,
        parameters: HashMap<String, Value>,
    ) -> CqlResult<Value> {
        // Wrap expression in minimal library structure
        let library_cql = format!(
            r#"library Adhoc version '1.0.0'

define Result:
  {}
"#,
            expression
        );

        // Parse CQL expression to AST
        let ast_library = parse(&library_cql).map_err(|e| CqlError::ParseError(format!("{:?}", e)))?;

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
        if let Some(ct) = context_type {
            if let Some(cv) = context_value {
                if let Some(cql_val) = super::data_provider::json_to_cql_value(&cv) {
                    ctx_builder = ctx_builder.context(ct, cql_val);
                }
            }
        }

        // Set parameters
        for (name, value) in parameters {
            if let Some(cql_val) = super::data_provider::json_to_cql_value(&value) {
                ctx_builder = ctx_builder.parameter(name, cql_val);
            }
        }

        let mut ctx = ctx_builder.build();

        // Evaluate the ELM library
        let results = self.engine.evaluate_library(&elm_library, &mut ctx).map_err(|e| {
            CqlError::EvaluationError(format!("{:?}", e))
        })?;

        // Get the "Result" definition from the library
        if let Some(result_value) = results.get("Result") {
            if let Some(json) = super::data_provider::cql_value_to_json(result_value) {
                Ok(json)
            } else {
                Ok(Value::Null)
            }
        } else {
            Ok(Value::Null)
        }
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

        tracing::info!(library_url = library_url, version = version, "Evaluating CQL library");

        let _library = self.library_cache.get_or_compile(library_url, version, &self.storage).await?;

        // Parse library CQL source to AST
        let cql_source = &_library.cql_source;
        let ast_library = parse(cql_source).map_err(|e| CqlError::ParseError(format!("{:?}", e)))?;

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

        if let Some(ct) = context_type {
            if let Some(cv) = context_value {
                if let Some(cql_val) = super::data_provider::json_to_cql_value(&cv) {
                    ctx_builder = ctx_builder.context(ct, cql_val);
                }
            }
        }

        for (name, value) in parameters {
            if let Some(cql_val) = super::data_provider::json_to_cql_value(&value) {
                ctx_builder = ctx_builder.parameter(name, cql_val);
            }
        }

        let mut ctx = ctx_builder.build();

        // Evaluate all definitions
        let results = self.engine.evaluate_library(&elm_library, &mut ctx).map_err(|e| {
            CqlError::EvaluationError(format!("{:?}", e))
        })?;

        // Convert to JSON
        let mut json_results = HashMap::new();
        for (name, value) in results {
            if let Some(json) = super::data_provider::cql_value_to_json(&value) {
                json_results.insert(name, json);
            }
        }

        Ok(json_results)
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

#[cfg(test)]
pub(crate) use tests::*;
