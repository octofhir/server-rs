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
//! With `debug` set to true the response also holds a debug trace (`traceStatus` and a `trace`
//! per event), and a failing expression answers with its trace and an `outcome`.
//!
//! # Examples
//!
//! System-level (no resource):
//! ```text
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
//! ```text
//! POST /fhir/$fhirpath
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "expression", "valueString": "Patient.name.given" },
//!     { "name": "resource", "resource": { "resourceType": "Patient", ... } }
//!   ]
//! }
//! ```

use crate::operations::handler::OperationLevel;

use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Instant;

use super::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_fhirpath::{Collection, DebugLimits, DebugTrace, FhirPathValue};

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

    /// Extract optional resource parameter.
    fn extract_resource(&self, params: &Value) -> Option<Value> {
        let parameters = params.get("parameter")?.as_array()?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some("resource")
                && let Some(resource) = param.get("resource")
            {
                return Some(resource.clone());
            }
        }

        None
    }

    /// Extract the optional `debug` parameter.
    fn extract_debug(&self, params: &Value) -> bool {
        params["parameter"].as_array().is_some_and(|parameters| {
            parameters
                .iter()
                .any(|parameter| parameter["name"] == "debug" && parameter["valueBoolean"] == true)
        })
    }

    /// The trace of a `debug=true` run. Unlike a run without it, a failing expression still
    /// answers with a Parameters: what it did up to the failure, and an `outcome`.
    fn add_trace(response: &mut Value, trace: &DebugTrace, failure: Option<&str>) {
        let parameters = response["parameter"]
            .as_array_mut()
            .expect("a Parameters resource has parameters");
        parameters.extend(trace.parameters());
        if let Some(failure) = failure {
            parameters.push(json!({"name": "outcome", "resource": {
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "error", "code": "processing", "diagnostics": failure}],
            }}));
        }
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
                        ("valueHumanName", json_value.to_json()),
                    ),
                    "Address" => (
                        type_name.to_string(),
                        ("valueAddress", json_value.to_json()),
                    ),
                    "Identifier" => (
                        type_name.to_string(),
                        ("valueIdentifier", json_value.to_json()),
                    ),
                    "CodeableConcept" => (
                        type_name.to_string(),
                        ("valueCodeableConcept", json_value.to_json()),
                    ),
                    "Coding" => (type_name.to_string(), ("valueCoding", json_value.to_json())),
                    "Reference" => (
                        type_name.to_string(),
                        ("valueReference", json_value.to_json()),
                    ),
                    "Period" => (type_name.to_string(), ("valuePeriod", json_value.to_json())),
                    "Range" => (type_name.to_string(), ("valueRange", json_value.to_json())),
                    "Ratio" => (type_name.to_string(), ("valueRatio", json_value.to_json())),
                    "ContactPoint" => (
                        type_name.to_string(),
                        ("valueContactPoint", json_value.to_json()),
                    ),
                    _ => (type_name.to_string(), ("resource", json_value.to_json())),
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
        debug: bool,
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
            state
                .terminology_provider
                .clone()
                .map(|h| h as std::sync::Arc<dyn octofhir_fhir_model::TerminologyProvider>),
            None, // validation provider
            None, // trace provider
        );

        // Measure evaluation time
        let eval_start = Instant::now();
        let (eval_result, trace) = if debug {
            let (result, trace) = state
                .fhirpath_engine
                .evaluate_debug(expression, &context, DebugLimits::default())
                .await;
            (result, Some(trace))
        } else {
            (
                state.fhirpath_engine.evaluate(expression, &context).await,
                None,
            )
        };
        let eval_time = eval_start.elapsed();

        let eval_result = match (eval_result, &trace) {
            (Ok(result), _) => result,
            (Err(error), Some(trace)) => {
                let mut response = json!({
                    "resourceType": "Parameters", "id": "fhirpath", "parameter": []
                });
                Self::add_trace(&mut response, trace, Some(&error.to_string()));
                return Ok(response);
            }
            (Err(error), None) => {
                return Err(OperationError::InvalidParameters(format!(
                    "FHIRPath evaluation error: {error}"
                )));
            }
        };

        // Extract Collection from EvaluationResult
        let result = eval_result.value;

        let mut response = self.build_response(
            expression,
            result,
            resource.as_ref().unwrap_or(&json!({})),
            parse_time,
            eval_time,
        );
        if let Some(trace) = &trace {
            Self::add_trace(&mut response, trace, None);
        }
        Ok(response)
    }
}

impl Default for FhirPathOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for FhirPathOperation {
    fn supports_level(&self, _level: OperationLevel, _resource_type: Option<&str>) -> bool {
        true
    }

    fn code(&self) -> &str {
        "fhirpath"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        _context: &crate::operations::OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;
        let resource = self.extract_resource(params);
        let debug = self.extract_debug(params);
        self.evaluate_expression(state, &expression, resource, debug)
            .await
    }

    async fn handle_type(
        &self,
        state: &AppState,
        _context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;
        let resource = self
            .extract_resource(params)
            .or_else(|| Some(json!({ "resourceType": resource_type })));
        let debug = self.extract_debug(params);
        self.evaluate_expression(state, &expression, resource, debug)
            .await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let expression = self.extract_expression(params)?;

        let resource = match self.extract_resource(params) {
            Some(res) => res,
            None => {
                let stored = context
                    .read(state, resource_type, id)
                    .await?
                    .ok_or_else(|| {
                        OperationError::NotFound(format!("{}/{} not found", resource_type, id))
                    })?;
                stored.resource
            }
        };

        let debug = self.extract_debug(params);
        self.evaluate_expression(state, &expression, Some(resource), debug)
            .await
    }
}
