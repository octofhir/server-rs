//! HTTP handlers for FHIR operations.
//!
//! This module provides axum handlers for routing FHIR operations at
//! system, type, and instance levels.
//!
//! # Routing Strategy
//!
//! FHIR operations are identified by the `$` prefix in the URL path:
//! - System level: `/$operation`
//! - Type level: `/{type}/$operation`
//! - Instance level: `/{type}/{id}/$operation`
//!
//! Since axum cannot distinguish between dynamic segments based on content,
//! we provide merged handlers that dispatch based on the `$` prefix.

use axum::{
    Json,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::collections::HashMap;

use super::params::OperationParams;
use crate::handlers;
use crate::server::AppState;
use octofhir_api::ApiError;

/// Checks if a path segment represents an operation (starts with `$`).
#[inline]
pub fn is_operation(segment: &str) -> bool {
    segment.starts_with('$')
}

/// Handler for system-level operations (e.g., `GET /$operation` or `POST /$operation`).
///
/// Routes operations like `/$meta`, `/$convert`, etc.
pub async fn system_operation_handler(
    State(state): State<AppState>,
    Path(operation): Path<String>,
    params: OperationParams,
) -> Result<impl IntoResponse, ApiError> {
    // Strip the $ prefix if present
    let code = operation.trim_start_matches('$');

    // Check if the operation is defined at system level
    let op_def = state.fhir_operations.get_system_operation(code);

    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${} not found at system level",
            code
        )));
    }

    // Look for a handler implementation
    let handler = state.operation_handlers.get(code);

    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_system(&state, &params_value)
                .await
                .map_err(ApiError::from)?;

            Ok((StatusCode::OK, Json(result)))
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${} is not implemented",
            code
        ))),
    }
}

/// Handler for type-level operations (e.g., `GET /Patient/$operation`).
///
/// Routes operations like `/Patient/$validate`, `/ValueSet/$expand`, etc.
pub async fn type_operation_handler(
    State(state): State<AppState>,
    Path((resource_type, operation)): Path<(String, String)>,
    params: OperationParams,
) -> Result<impl IntoResponse, ApiError> {
    let code = operation.trim_start_matches('$');

    // Check if the operation is defined at type level for this resource type
    let op_def = state
        .fhir_operations
        .get_type_operation(&resource_type, code);

    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${} not found for type {}",
            code, resource_type
        )));
    }

    // Look for a handler implementation
    let handler = state.operation_handlers.get(code);

    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_type(&state, &resource_type, &params_value)
                .await
                .map_err(ApiError::from)?;

            Ok((StatusCode::OK, Json(result)))
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${} is not implemented",
            code
        ))),
    }
}

/// Handler for instance-level operations (e.g., `GET /Patient/123/$operation`).
///
/// Routes operations like `/Patient/123/$validate`, `/Patient/123/$everything`, etc.
///
/// # Note
/// This handler validates that the operation parameter starts with `$`.
/// If it doesn't, a 404 is returned indicating the path is not a valid operation.
/// Combined GET handler for instance-level operations and history.
///
/// Dispatches `_history` requests to the history handler and `$operation` requests
/// to the operation handler. These must share a route because matchit cannot have
/// both `/{a}/{b}/_history` and `/{a}/{b}/{c}` as separate routes.
pub async fn instance_operation_or_history_handler(
    state: State<AppState>,
    Path((resource_type, id, operation)): Path<(String, String, String)>,
    Query(query_params): Query<HashMap<String, String>>,
    query: Query<crate::handlers::HistoryQueryParams>,
) -> Response {
    if operation == "_history" {
        let path = Path((resource_type, id));
        match crate::handlers::instance_history(state, path, query).await {
            Ok(resp) => resp.into_response(),
            Err(e) => e.into_response(),
        }
    } else if is_operation(&operation) {
        let app_state = state.0;
        let code = operation.trim_start_matches('$');
        let op_def = app_state
            .fhir_operations
            .get_instance_operation(&resource_type, code);
        if op_def.is_none() {
            return ApiError::not_found(format!(
                "Operation ${code} not found for {resource_type}/{id}"
            ))
            .into_response();
        }
        let handler = app_state.operation_handlers.get(code);
        match handler {
            Some(h) => {
                let params = OperationParams::Get(query_params);
                let params_value = params.to_value();
                match h
                    .handle_instance(&app_state, &resource_type, &id, &params_value)
                    .await
                {
                    Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                    Err(e) => ApiError::from(e).into_response(),
                }
            }
            None => ApiError::not_implemented(format!("Operation ${code} is not implemented"))
                .into_response(),
        }
    } else {
        ApiError::not_found(format!(
            "Invalid path: /{resource_type}/{id}/{operation}. Operations must start with '$'"
        ))
        .into_response()
    }
}

pub async fn instance_operation_handler(
    State(state): State<AppState>,
    Path((resource_type, id, operation)): Path<(String, String, String)>,
    params: OperationParams,
) -> Result<impl IntoResponse, ApiError> {
    // Validate that this is actually an operation (starts with $)
    if !is_operation(&operation) {
        return Err(ApiError::not_found(format!(
            "Invalid operation path: /{}/{}/{}. Operations must start with '$'",
            resource_type, id, operation
        )));
    }

    let code = operation.trim_start_matches('$');

    // Check if the operation is defined at instance level for this resource type
    let op_def = state
        .fhir_operations
        .get_instance_operation(&resource_type, code);

    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${} not found for {}/{}",
            code, resource_type, id
        )));
    }

    // Look for a handler implementation
    let handler = state.operation_handlers.get(code);

    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_instance(&state, &resource_type, &id, &params_value)
                .await
                .map_err(ApiError::from)?;

            Ok((StatusCode::OK, Json(result)))
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${} is not implemented",
            code
        ))),
    }
}

// =============================================================================
// Merged Handlers
// =============================================================================

/// Merged handler for GET `/{param}` route that dispatches to either:
/// - System-level operation handler if `param` starts with `$`
/// - Resource search handler otherwise
///
/// This allows a single route to handle both `/$meta` and `/Patient`.
pub async fn merged_root_get_handler(
    state: State<AppState>,
    headers: HeaderMap,
    Path(param): Path<String>,
    Query(query_params): Query<HashMap<String, String>>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    if is_operation(&param) {
        // Dispatch to system operation handler
        let params = OperationParams::Get(query_params.clone());
        let result = system_operation_handler_internal(state, param, params).await?;
        Ok(result.into_response())
    } else {
        // Dispatch to resource search handler
        let result = handlers::search_resource(
            state,
            headers,
            Path(param),
            Query(query_params),
            RawQuery(raw),
        )
        .await?;
        Ok(result.into_response())
    }
}

/// Merged handler for POST `/{param}` route that dispatches to either:
/// - System-level operation handler if `param` starts with `$`
/// - Resource create handler otherwise
pub async fn merged_root_post_handler(
    state: State<AppState>,
    Path(param): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    if is_operation(&param) {
        // Parse body as JSON for operation (empty body → null)
        let value: serde_json::Value = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body)
                .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?
        };
        let params = OperationParams::Post(value);
        let result = system_operation_handler_internal(state, param, params).await?;
        Ok(result.into_response())
    } else {
        // Dispatch to resource create handler - need to reconstruct JSON
        let json_value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?;
        let result =
            handlers::create_resource(state, Path(param), headers, Json(json_value)).await?;
        Ok(result.into_response())
    }
}

/// Merged handler for GET `/{resource_type}/{param}` route that dispatches to either:
/// - Type-level operation handler if `param` starts with `$`
/// - Resource read handler otherwise
pub async fn merged_type_get_handler(
    state: State<AppState>,
    Path((resource_type, param)): Path<(String, String)>,
    Query(query_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if is_operation(&param) {
        // Dispatch to type operation handler
        let params = OperationParams::Get(query_params);
        let result = type_operation_handler_internal(state, resource_type, param, params).await?;
        Ok(result.into_response())
    } else {
        // Dispatch to resource read handler (param is the resource id)
        let result = handlers::read_resource(state, Path((resource_type, param)), headers).await?;
        Ok(result.into_response())
    }
}

/// Merged handler for POST `/{resource_type}/{param}` route that dispatches to:
/// - Type-level operation handler if `param` starts with `$`
/// - Returns 405 Method Not Allowed otherwise (POST to /{type}/{id} is not valid)
pub async fn merged_type_post_handler(
    state: State<AppState>,
    Path((resource_type, param)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    if is_operation(&param) {
        // Parse body as JSON for operation (empty body → null)
        let value: serde_json::Value = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body)
                .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?
        };
        let params = OperationParams::Post(value);
        let result = type_operation_handler_internal(state, resource_type, param, params).await?;
        Ok(result.into_response())
    } else {
        // POST to /{type}/{id} is not a valid FHIR operation
        Err(ApiError::bad_request(format!(
            "POST to /{}/{} is not supported. Use PUT to update a resource.",
            resource_type, param
        )))
    }
}

/// POST handler for compartment routes like `/Patient/{id}/{param}`.
///
/// Compartment routes (e.g. `/Patient/{id}/{resource_type}`) match before the
/// generic `/{resource_type}/{id}/{operation}` route. This POST handler dispatches
/// `$operation` requests to the instance-level operation handler.
pub async fn compartment_post_handler(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<AppState>,
    Path((id, param)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    if is_operation(&param) {
        // Extract compartment type from URI: /fhir/Patient/123/$reindex → "Patient"
        let compartment_type = uri
            .path()
            .strip_prefix("/fhir/")
            .unwrap_or(uri.path())
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();

        let value: serde_json::Value = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body)
                .map_err(|e| ApiError::bad_request(format!("Invalid JSON: {}", e)))?
        };
        let params = OperationParams::Post(value);
        instance_operation_handler_internal(&state, &compartment_type, &id, &param, params).await
    } else {
        Err(ApiError::bad_request(format!(
            "POST to this compartment path is not supported."
        )))
    }
}

// =============================================================================
// Internal Helper Functions
// =============================================================================

/// Internal system operation handler that takes pre-parsed parameters.
async fn system_operation_handler_internal(
    State(state): State<AppState>,
    operation: String,
    params: OperationParams,
) -> Result<impl IntoResponse, ApiError> {
    let code = operation.trim_start_matches('$');

    let op_def = state.fhir_operations.get_system_operation(code);
    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${} not found at system level",
            code
        )));
    }

    let handler = state.operation_handlers.get(code);
    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_system(&state, &params_value)
                .await
                .map_err(ApiError::from)?;
            Ok((StatusCode::OK, Json(result)))
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${} is not implemented",
            code
        ))),
    }
}

/// Internal instance operation handler that takes pre-parsed parameters.
async fn instance_operation_handler_internal(
    state: &AppState,
    resource_type: &str,
    id: &str,
    operation: &str,
    params: OperationParams,
) -> Result<Response, ApiError> {
    let code = operation.trim_start_matches('$');

    let op_def = state
        .fhir_operations
        .get_instance_operation(resource_type, code);
    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${code} not found for {resource_type}/{id}"
        )));
    }

    let handler = state.operation_handlers.get(code);
    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_instance(state, resource_type, id, &params_value)
                .await
                .map_err(ApiError::from)?;
            Ok((StatusCode::OK, Json(result)).into_response())
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${code} is not implemented"
        ))),
    }
}

/// Internal type operation handler that takes pre-parsed parameters.
async fn type_operation_handler_internal(
    State(state): State<AppState>,
    resource_type: String,
    operation: String,
    params: OperationParams,
) -> Result<impl IntoResponse, ApiError> {
    let code = operation.trim_start_matches('$');

    let op_def = state
        .fhir_operations
        .get_type_operation(&resource_type, code);
    if op_def.is_none() {
        return Err(ApiError::not_found(format!(
            "Operation ${} not found for type {}",
            code, resource_type
        )));
    }

    let handler = state.operation_handlers.get(code);
    match handler {
        Some(h) => {
            let params_value = params.to_value();
            let result = h
                .handle_type(&state, &resource_type, &params_value)
                .await
                .map_err(ApiError::from)?;
            Ok((StatusCode::OK, Json(result)))
        }
        None => Err(ApiError::not_implemented(format!(
            "Operation ${} is not implemented",
            code
        ))),
    }
}
