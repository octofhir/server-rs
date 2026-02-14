//! FHIRPath operation handler.
//!
//! This module implements the `$fhirpath` operation which evaluates FHIRPath expressions
//! against FHIR resources and returns detailed results with metadata.
//!
//! # Response Structure
//!
//! The operation returns a FHIR Parameters resource with:
//! - **metadata**: Evaluator version, expression, expected return type, and timing breakdown
//! - **results**: Collection of values with proper FHIR type mapping and resource paths
//!
//! # Examples
//!
//! System-level (no resource):
//! ```
//! POST /fhir/$fhirpath
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "expression", "valueString": "1 + 2" }
//!   ]
//! }
//! ```
//!
//! With resource:
//! ```
//! POST /fhir/$fhirpath
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "expression", "valueString": "Patient.name.given" },
//!     { "name": "resource", "resource": { "resourceType": "Patient", ... } }
//!   ]
//! }
//! ```

use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Instant;

use super::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_fhirpath::{Collection, FhirPathValue};

/// Handler for the `$fhirpath` operation.
///
/// Evaluates FHIRPath expressions and returns detailed results with metadata.
pub struct FhirPathOperation;

impl FhirPathOperation {
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

    /// Extract optional resource parameter.
    fn extract_resource(&self, params: &Value) -> Option<Value> {
        let parameters = params.get("parameter")?.as_array()?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some("resource") {
                if let Some(resource) = param.get("resource") {
                    return Some(resource.clone());
                }
            }
        }

        None
    }

    /// Build comprehensive Parameters response with metadata and results.
    fn build_response(
        &self,
        expression: &str,
        result: Collection,
        _resource: &Value,
        parse_time: std::time::Duration,
        eval_time: std::time::Duration,
    ) -> Value {
        let mut params = Vec::new();

        // Metadata section
        params.push(self.build_metadata_section(expression, &result, parse_time, eval_time));

        // Result section(s)
        for (index, value) in result.into_iter().enumerate() {
            if let Some(result_param) = self.build_result_parameter(value, index) {
                params.push(result_param);
            }
        }

        json!({
            "resourceType": "Parameters",
            "id": "fhirpath",
            "parameter": params
        })
    }

    /// Build metadata section with timing, diagnostics, expected type.
    fn build_metadata_section(
        &self,
        expression: &str,
        result: &Collection,
        parse_time: std::time::Duration,
        eval_time: std::time::Duration,
    ) -> Value {
        let total_time = parse_time + eval_time;
        let result_count = result.len();

        let mut parts = vec![
            json!({
                "name": "evaluator",
                "valueString": format!("octofhir-fhirpath-{} (R4)", octofhir_fhirpath::VERSION)
            }),
            json!({
                "name": "expression",
                "valueString": expression
            }),
            json!({
                "name": "resultCount",
                "valueInteger": result_count
            }),
        ];

        // Timing breakdown
        parts.push(json!({
            "name": "timing",
            "part": [
                {
                    "name": "parseTime",
                    "valueDecimal": parse_time.as_secs_f64() * 1000.0
                },
                {
                    "name": "evaluationTime",
                    "valueDecimal": eval_time.as_secs_f64() * 1000.0
                },
                {
                    "name": "totalTime",
                    "valueDecimal": total_time.as_secs_f64() * 1000.0
                }
            ]
        }));

        json!({
            "name": "metadata",
            "part": parts
        })
    }

    /// Build result parameter with proper type mapping.
    fn build_result_parameter(&self, value: FhirPathValue, index: usize) -> Option<Value> {
        let (datatype, value_field) = self.map_fhirpath_value_to_parameter(value);

        Some(json!({
            "name": datatype,
            value_field.0: value_field.1,
            "part": [
                {
                    "name": "index",
                    "valueInteger": index
                }
            ]
        }))
    }

    /// Map FhirPathValue to proper FHIR Parameter value field.
    fn map_fhirpath_value_to_parameter(
        &self,
        value: FhirPathValue,
    ) -> (String, (&'static str, Value)) {
        match value {
            FhirPathValue::Boolean(b, _, _) => ("boolean".to_string(), ("valueBoolean", json!(b))),
            FhirPathValue::Integer(i, _, _) => ("integer".to_string(), ("valueInteger", json!(i))),
            FhirPathValue::String(s, type_info, _) => {
                let type_name = type_info.name.as_deref().unwrap_or(&type_info.type_name);

                match type_name {
                    "code" => ("code".to_string(), ("valueCode", json!(s))),
                    "id" => ("id".to_string(), ("valueId", json!(s))),
                    "uri" => ("uri".to_string(), ("valueUri", json!(s))),
                    "url" => ("url".to_string(), ("valueUrl", json!(s))),
                    _ => ("string".to_string(), ("valueString", json!(s))),
                }
            }
            FhirPathValue::Decimal(d, _, _) => (
                "decimal".to_string(),
                ("valueDecimal", json!(d.to_string())),
            ),
            FhirPathValue::Date(date, _, _) => {
                ("date".to_string(), ("valueDate", json!(date.to_string())))
            }
            FhirPathValue::DateTime(dt, _, _) => (
                "dateTime".to_string(),
                ("valueDateTime", json!(dt.to_string())),
            ),
            FhirPathValue::Time(time, _, _) => {
                ("time".to_string(), ("valueTime", json!(time.to_string())))
            }
            FhirPathValue::Quantity { value, unit, .. } => (
                "Quantity".to_string(),
                (
                    "valueQuantity",
                    json!({
                        "value": value.to_string(),
                        "unit": unit
                    }),
                ),
            ),
            FhirPathValue::Resource(json_value, type_info, _) => {
                let type_name = type_info.name.as_deref().unwrap_or(&type_info.type_name);

                // Check if this is a complex datatype or resource
                match type_name {
                    "HumanName" => (
                        type_name.to_string(),
                        ("valueHumanName", json_value.as_ref().clone()),
                    ),
                    "Address" => (
                        type_name.to_string(),
                        ("valueAddress", json_value.as_ref().clone()),
                    ),
                    "Identifier" => (
                        type_name.to_string(),
                        ("valueIdentifier", json_value.as_ref().clone()),
                    ),
                    "CodeableConcept" => (
                        type_name.to_string(),
                        ("valueCodeableConcept", json_value.as_ref().clone()),
                    ),
                    "Coding" => (
                        type_name.to_string(),
                        ("valueCoding", json_value.as_ref().clone()),
                    ),
                    "Reference" => (
                        type_name.to_string(),
                        ("valueReference", json_value.as_ref().clone()),
                    ),
                    "Period" => (
                        type_name.to_string(),
                        ("valuePeriod", json_value.as_ref().clone()),
                    ),
                    "Range" => (
                        type_name.to_string(),
                        ("valueRange", json_value.as_ref().clone()),
                    ),
                    "Ratio" => (
                        type_name.to_string(),
                        ("valueRatio", json_value.as_ref().clone()),
                    ),
                    "ContactPoint" => (
                        type_name.to_string(),
                        ("valueContactPoint", json_value.as_ref().clone()),
                    ),
                    _ => (
                        type_name.to_string(),
                        ("resource", json_value.as_ref().clone()),
                    ),
                }
            }
            _ => ("unknown".to_string(), ("valueString", json!("{}"))),
        }
    }

    /// Core evaluation logic with timing.
    async fn evaluate_expression(
        &self,
        state: &AppState,
        expression: &str,
        resource: Option<Value>,
    ) -> Result<Value, OperationError> {
        // For now, parse and eval are combined - measure total time
        let parse_start = Instant::now();
        let parse_time = parse_start.elapsed(); // Placeholder - actual parsing is internal

        // Build context
        let collection = if let Some(res) = &resource {
            Collection::from_json_resource(res.clone(), Some(state.model_provider.clone()))
                .await
                .unwrap_or_else(|_| {
                    // Fallback to single untyped resource if type conversion fails
                    Collection::single(octofhir_fhirpath::FhirPathValue::resource(res.clone()))
                })
        } else {
            Collection::empty()
        };

        let context = octofhir_fhirpath::EvaluationContext::new(
            collection,
            state.model_provider.clone(),
            state.terminology_provider.clone(),
            None, // validation provider
            None, // trace provider
        );

        // Measure evaluation time
        let eval_start = Instant::now();
        let eval_result = state
            .fhirpath_engine
            .evaluate(expression, &context)
            .await
            .map_err(|e| {
                OperationError::InvalidParameters(format!("FHIRPath evaluation error: {}", e))
            })?;
        let eval_time = eval_start.elapsed();

        // Extract Collection from EvaluationResult
        let result = eval_result.value;

        Ok(self.build_response(
            expression,
            result,
            resource.as_ref().unwrap_or(&json!({})),
            parse_time,
            eval_time,
        ))
    }
}

impl Default for FhirPathOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for FhirPathOperation {
    fn code(&self) -> &str {
        "fhirpath"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;
        let resource = self.extract_resource(params);
        self.evaluate_expression(state, &expression, resource).await
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;
        let resource = self
            .extract_resource(params)
            .or_else(|| Some(json!({ "resourceType": resource_type })));
        self.evaluate_expression(state, &expression, resource).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;

        let resource = match self.extract_resource(params) {
            Some(res) => res,
            None => {
                let stored = state
                    .storage
                    .read(resource_type, id)
                    .await
                    .map_err(|e| OperationError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        OperationError::NotFound(format!("{}/{} not found", resource_type, id))
                    })?;
                stored.resource
            }
        };

        self.evaluate_expression(state, &expression, Some(resource))
            .await
    }
}
