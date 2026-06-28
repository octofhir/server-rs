//! CQL operation handler.
//!
//! This module implements the `$cql` operation which evaluates CQL expressions
//! against FHIR resources and returns detailed results with metadata.
//!
//! # Response Structure
//!
//! The operation returns a FHIR Parameters resource with:
//! - **expression**: The CQL expression that was evaluated
//! - **result**: The evaluation result serialized as a JSON value
//!
//! # Examples
//!
//! System-level (no resource context):
//! ```
//! POST /fhir/$cql
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "expression", "valueString": "1 + 1" }
//!   ]
//! }
//! ```
//!
//! Instance-level (with resource context):
//! ```
//! POST /fhir/Patient/123/$cql
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "expression", "valueString": "Patient.birthDate" }
//!   ]
//! }
//! ```

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::collections::HashMap;

use super::{OperationError, OperationHandler};
use crate::server::AppState;

/// Map a CQL service error onto an operation error, preserving the real
/// diagnostic message. Parse / evaluation / invalid-input failures are client
/// errors (400); infrastructure failures (timeout, storage) are 500s.
fn map_cql_error(err: octofhir_cql_service::CqlError) -> OperationError {
    use octofhir_cql_service::CqlError;
    let message = err.to_string();
    match err {
        CqlError::ParseError(_)
        | CqlError::EvaluationError(_)
        | CqlError::CompilationError(_)
        | CqlError::InvalidParameter(_)
        | CqlError::LibraryNotFound(_) => OperationError::InvalidParameters(message),
        _ => OperationError::Internal(message),
    }
}

/// Handler for the `$cql` operation.
///
/// Evaluates CQL expressions and returns detailed results with metadata.
pub struct CqlOperation;

impl CqlOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract expression parameter (required).
    fn extract_expression(&self, params: &Value) -> Result<String, OperationError> {
        let parameters = params
            .get("parameter")
            .and_then(|p| p.as_array())
            .ok_or_else(|| {
                OperationError::InvalidParameters("Missing parameter array".to_string())
            })?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some("expression")
                && let Some(expr) = param.get("valueString").and_then(|v| v.as_str())
            {
                return Ok(expr.to_string());
            }
        }

        Err(OperationError::InvalidParameters(
            "Missing required parameter: expression".to_string(),
        ))
    }

    /// Names handled out-of-band (expression source, library source, context)
    /// rather than as CQL `parameter` declarations.
    const RESERVED_PARAMS: [&'static str; 5] = [
        "expression",
        "library",
        "context",
        "contextValue",
        "validate",
    ];

    /// Whether the request asks for parse-only validation (`validate` = true).
    fn wants_validation(&self, params: &Value) -> bool {
        params
            .get("parameter")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter().any(|p| {
                    p.get("name").and_then(|n| n.as_str()) == Some("validate")
                        && p.get("valueBoolean").and_then(|v| v.as_bool()) == Some(true)
                })
            })
            .unwrap_or(false)
    }

    /// Resolve the full CQL source to parse: an inline `library` as-is, otherwise
    /// the `expression` wrapped in a minimal ad-hoc library.
    fn resolve_source(&self, params: &Value) -> Result<String, OperationError> {
        if let Some(library) = self.extract_library(params) {
            return Ok(library);
        }
        let expression = self.extract_expression(params)?;
        Ok(format!(
            "library Adhoc version '1.0.0'\ndefine Result:\n  {}",
            expression
        ))
    }

    /// Extract the optional inline CQL library source (`library` parameter).
    ///
    /// When present, the full source is evaluated as a library and every
    /// `define` statement is returned — the multi-define workbench mode.
    fn extract_library(&self, params: &Value) -> Option<String> {
        params
            .get("parameter")
            .and_then(|p| p.as_array())?
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("library"))
            .and_then(|p| p.get("valueString").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    }

    /// Extract the evaluation context (`context` valueCode + `contextValue`
    /// resource). Falls back to the resource's own `resourceType` when no
    /// explicit `context` code is supplied.
    fn extract_context(&self, params: &Value) -> (Option<String>, Option<Value>) {
        let mut context_type = None;
        let mut context_value = None;

        if let Some(parameters) = params.get("parameter").and_then(|p| p.as_array()) {
            for param in parameters {
                match param.get("name").and_then(|n| n.as_str()) {
                    Some("context") => {
                        context_type = param
                            .get("valueCode")
                            .or_else(|| param.get("valueString"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                    Some("contextValue") => {
                        context_value = param.get("resource").cloned();
                    }
                    _ => {}
                }
            }
        }

        if context_type.is_none() {
            if let Some(cv) = &context_value {
                context_type = cv
                    .get("resourceType")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }

        (context_type, context_value)
    }

    /// Extract optional parameters from Parameters resource
    fn extract_parameters(&self, params: &Value) -> HashMap<String, Value> {
        let mut result = HashMap::new();

        if let Some(parameters) = params.get("parameter").and_then(|p| p.as_array()) {
            for param in parameters {
                if let Some(name) = param.get("name").and_then(|n| n.as_str()) {
                    // Skip reserved parameters handled separately
                    if Self::RESERVED_PARAMS.contains(&name) {
                        continue;
                    }

                    // Extract value based on type
                    if let Some(value) = param.get("valueString") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("valueInteger") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("valueBoolean") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("valueDecimal") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("valueDate") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("valueDateTime") {
                        result.insert(name.to_string(), value.clone());
                    } else if let Some(value) = param.get("resource") {
                        result.insert(name.to_string(), value.clone());
                    }
                }
            }
        }

        result
    }

    /// Build Parameters response for a single ad-hoc expression.
    fn build_response(&self, expression: &str, result: Value) -> Value {
        json!({
            "resourceType": "Parameters",
            "id": "cql",
            "parameter": [
                {
                    "name": "expression",
                    "valueString": expression
                },
                {
                    "name": "return",
                    "valueString": serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string())
                }
            ]
        })
    }

    /// Build Parameters response for a full library evaluation: one `part` per
    /// `define`, in source order, each carrying the JSON-serialized value.
    fn build_library_response(&self, defines: IndexMap<String, Value>) -> Value {
        let parts: Vec<Value> = defines
            .into_iter()
            .map(|(name, value)| {
                json!({
                    "name": name,
                    "valueString": serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
                })
            })
            .collect();

        json!({
            "resourceType": "Parameters",
            "id": "cql",
            "parameter": [
                {
                    "name": "result",
                    "part": parts
                }
            ]
        })
    }

    /// Build Parameters response for parse-only validation: one `issue` part per
    /// diagnostic. An empty `parameter` list means the source is valid.
    fn build_validate_response(&self, issues: Vec<octofhir_cql_service::ValidationIssue>) -> Value {
        let parameter: Vec<Value> = issues
            .into_iter()
            .map(|issue| {
                let mut part = vec![
                    json!({ "name": "severity", "valueString": issue.severity }),
                    json!({ "name": "message", "valueString": issue.message }),
                ];
                if let Some(line) = issue.line {
                    part.push(json!({ "name": "line", "valueInteger": line }));
                }
                if let Some(column) = issue.column {
                    part.push(json!({ "name": "column", "valueInteger": column }));
                }
                json!({ "name": "issue", "part": part })
            })
            .collect();

        json!({
            "resourceType": "Parameters",
            "id": "cql-validate",
            "parameter": parameter
        })
    }

    /// Shared dispatch: validate-only, or evaluate either an inline `library`
    /// (multi-define) or a single `expression`, with the given context.
    async fn run(
        &self,
        state: &AppState,
        context_type: Option<&str>,
        context_value: Option<Value>,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let cql_service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".to_string()))?;

        // Parse-only validation — no evaluation, no data access.
        if self.wants_validation(params) {
            let source = self.resolve_source(params)?;
            let issues = cql_service.validate_source(&source);
            return Ok(self.build_validate_response(issues));
        }

        let parameters = self.extract_parameters(params);

        if let Some(library_source) = self.extract_library(params) {
            let defines = cql_service
                .evaluate_library_source(&library_source, context_type, context_value, parameters)
                .await
                .map_err(map_cql_error)?;
            return Ok(self.build_library_response(defines));
        }

        let expression = self.extract_expression(params)?;
        let result = cql_service
            .evaluate_expression(&expression, context_type, context_value, parameters)
            .await
            .map_err(map_cql_error)?;
        Ok(self.build_response(&expression, result))
    }
}

impl Default for CqlOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for CqlOperation {
    fn code(&self) -> &str {
        "cql"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Context comes purely from the request body (inline `contextValue`).
        let (context_type, context_value) = self.extract_context(params);
        self.run(state, context_type.as_deref(), context_value, params)
            .await
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Explicit body context wins; otherwise default to the path resource type.
        let (body_type, context_value) = self.extract_context(params);
        let context_type = body_type.unwrap_or_else(|| resource_type.to_string());
        self.run(state, Some(&context_type), context_value, params)
            .await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Retrieve the resource and use it as the evaluation context.
        let resource = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        self.run(state, Some(resource_type), Some(resource.resource), params)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_expression() {
        let operation = CqlOperation::new();

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "expression",
                    "valueString": "1 + 1"
                }
            ]
        });

        let expression = operation.extract_expression(&params).unwrap();
        assert_eq!(expression, "1 + 1");
    }

    #[test]
    fn test_extract_expression_missing() {
        let operation = CqlOperation::new();

        let params = json!({
            "resourceType": "Parameters",
            "parameter": []
        });

        let result = operation.extract_expression(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_parameters() {
        let operation = CqlOperation::new();

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "expression",
                    "valueString": "foo"
                },
                {
                    "name": "param1",
                    "valueString": "bar"
                },
                {
                    "name": "param2",
                    "valueInteger": 42
                }
            ]
        });

        let parameters = operation.extract_parameters(&params);
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters.get("param1").unwrap(), "bar");
        assert_eq!(parameters.get("param2").unwrap(), 42);
    }
}
