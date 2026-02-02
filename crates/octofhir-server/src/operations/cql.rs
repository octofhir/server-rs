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
use serde_json::{Value, json};
use std::collections::HashMap;

use super::{OperationError, OperationHandler};
use crate::server::AppState;

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
            if param.get("name").and_then(|n| n.as_str()) == Some("expression") {
                if let Some(expr) = param.get("valueString").and_then(|v| v.as_str()) {
                    return Ok(expr.to_string());
                }
            }
        }

        Err(OperationError::InvalidParameters(
            "Missing required parameter: expression".to_string(),
        ))
    }

    /// Extract optional parameters from Parameters resource
    fn extract_parameters(&self, params: &Value) -> HashMap<String, Value> {
        let mut result = HashMap::new();

        if let Some(parameters) = params.get("parameter").and_then(|p| p.as_array()) {
            for param in parameters {
                if let Some(name) = param.get("name").and_then(|n| n.as_str()) {
                    // Skip expression parameter as it's handled separately
                    if name == "expression" {
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

    /// Build Parameters response with result
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
        // Get CQL service from app state
        let cql_service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".to_string()))?;

        // Extract expression and parameters
        let expression = self.extract_expression(params)?;
        let parameters = self.extract_parameters(params);

        // Evaluate expression (no resource context)
        let result = cql_service
            .evaluate_expression(&expression, None, None, parameters)
            .await
            .map_err(|e| OperationError::Internal(format!("CQL evaluation failed: {}", e)))?;

        Ok(self.build_response(&expression, result))
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Get CQL service from app state
        let cql_service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".to_string()))?;

        // Extract expression and parameters
        let expression = self.extract_expression(params)?;
        let parameters = self.extract_parameters(params);

        // Evaluate expression with resource type context
        let result = cql_service
            .evaluate_expression(&expression, Some(resource_type), None, parameters)
            .await
            .map_err(|e| OperationError::Internal(format!("CQL evaluation failed: {}", e)))?;

        Ok(self.build_response(&expression, result))
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Get CQL service from app state
        let cql_service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".to_string()))?;

        // Retrieve resource as context
        let resource = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Extract expression and parameters
        let expression = self.extract_expression(params)?;
        let parameters = self.extract_parameters(params);

        // Evaluate expression with resource context
        let result = cql_service
            .evaluate_expression(
                &expression,
                Some(resource_type),
                Some(resource.resource),
                parameters,
            )
            .await
            .map_err(|e| OperationError::Internal(format!("CQL evaluation failed: {}", e)))?;

        Ok(self.build_response(&expression, result))
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
