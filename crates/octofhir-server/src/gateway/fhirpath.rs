//! FHIRPath handler for evaluating FHIRPath expressions.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use tracing::{debug, info, instrument, warn};

use super::error::GatewayError;
use super::types::CustomOperation;
use crate::server::AppState;

/// Handles FHIRPath operations by evaluating expressions against request data.
///
/// This handler:
/// 1. Extracts FHIRPath expression from the operation
/// 2. Parses request body as the evaluation context
/// 3. Evaluates the FHIRPath expression
/// 4. Returns the result as JSON
///
/// # Example
///
/// Given a request body:
/// ```json
/// {
///   "resourceType": "Patient",
///   "name": [{"given": ["John"], "family": "Doe"}]
/// }
/// ```
///
/// And FHIRPath expression: `name.given`
///
/// Returns: `["John"]`
#[instrument(skip(state, operation, request))]
pub async fn handle_fhirpath(
    state: &AppState,
    operation: &CustomOperation,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let fhirpath_expr = operation.fhirpath.as_ref().ok_or_else(|| {
        GatewayError::InvalidConfig("FHIRPath operation missing fhirpath configuration".to_string())
    })?;

    info!(expression = %fhirpath_expr, "Evaluating FHIRPath expression");

    // Read request body as JSON (this will be the evaluation context)
    let body_bytes = axum::body::to_bytes(request.into_body(), 10_000_000)
        .await
        .map_err(|e| GatewayError::FhirPathError(format!("Failed to read request body: {}", e)))?;

    let context: Value = if body_bytes.is_empty() {
        // If no body provided, use empty object
        json!({})
    } else {
        serde_json::from_slice(&body_bytes)
            .map_err(|e| GatewayError::FhirPathError(format!("Invalid JSON in request body: {}", e)))?
    };

    debug!(context = ?context, "Evaluation context");

    // Create evaluation context from JSON
    use octofhir_fhirpath::{Collection, EvaluationContext};

    let fhirpath_provider: Arc<dyn octofhir_fhirpath::ModelProvider + Send + Sync> =
        state.model_provider.clone();
    let collection = Collection::from_json_resource(context, Some(fhirpath_provider.clone()))
        .await
        .map_err(|e| GatewayError::FhirPathError(format!("Failed to create FHIRPath context: {}", e)))?;
    let eval_context = EvaluationContext::new(collection, fhirpath_provider, None, None, None);

    // Evaluate FHIRPath expression
    match state
        .fhirpath_engine
        .evaluate(fhirpath_expr, &eval_context)
        .await
    {
        Ok(result) => {
            info!("FHIRPath evaluation succeeded");

            // Convert FHIRPath result to JSON
            // EvaluationResult has a value field that contains the result collection
            let values_json: Vec<Value> = result
                .value
                .iter()
                .map(|v| v.to_json_value())
                .collect();

            let json_result = json!(values_json);

            Ok((StatusCode::OK, Json(json_result)).into_response())
        }
        Err(e) => {
            warn!(error = %e, "FHIRPath evaluation failed");
            Err(GatewayError::FhirPathError(format!(
                "FHIRPath evaluation failed: {}",
                e
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests would require setting up a full AppState with FHIRPath engine
    // Placeholder for now
    #[test]
    fn test_placeholder() {
        // Actual tests require FHIRPath engine initialization
    }
}
