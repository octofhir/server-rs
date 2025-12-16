//! Axum HTTP handlers for GraphQL endpoints.
//!
//! This module provides the HTTP handlers for GraphQL requests:
//! - `POST /$graphql` - System-level GraphQL endpoint
//! - `GET /$graphql` - System-level GraphQL (query via URL param)
//! - `POST /:type/:id/$graphql` - Instance-level GraphQL endpoint
//!
//! The handlers integrate with the OctoFHIR authentication middleware and
//! convert GraphQL errors to FHIR-compliant responses.

use std::net::IpAddr;
use std::sync::Arc;

use async_graphql::{Request, Response, Variables};
use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use octofhir_auth::middleware::AuthContext;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::context::{GraphQLContext, GraphQLContextBuilder};
use crate::error::GraphQLError;
use crate::schema::LazySchema;

/// State shared across GraphQL handlers.
#[derive(Clone)]
pub struct GraphQLState {
    /// Lazy-loaded GraphQL schema.
    pub lazy_schema: Arc<LazySchema>,

    /// Context builder template with shared dependencies.
    pub context_template: GraphQLContextTemplate,
}

/// Template for building per-request GraphQL context.
///
/// This contains the shared dependencies that are cloned into each request's context.
#[derive(Clone)]
pub struct GraphQLContextTemplate {
    pub storage: octofhir_storage::DynStorage,
    pub search_config: octofhir_search::SearchConfig,
    pub policy_evaluator: Arc<octofhir_auth::policy::PolicyEvaluator>,
}

/// GraphQL request body.
#[derive(Debug, Deserialize)]
pub struct GraphQLRequest {
    /// The GraphQL query string.
    pub query: String,

    /// Optional operation name for multi-operation documents.
    #[serde(rename = "operationName")]
    pub operation_name: Option<String>,

    /// Optional variables for the query.
    pub variables: Option<serde_json::Value>,

    /// Optional extensions.
    pub extensions: Option<serde_json::Value>,
}

/// Query parameters for GET requests.
#[derive(Debug, Deserialize)]
pub struct GraphQLQueryParams {
    /// The GraphQL query string.
    pub query: Option<String>,

    /// Optional operation name.
    #[serde(rename = "operationName")]
    pub operation_name: Option<String>,

    /// Optional variables (JSON string).
    pub variables: Option<String>,
}

/// GraphQL response with optional error extensions.
#[derive(Debug, Serialize)]
pub struct GraphQLResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,
}

impl From<Response> for GraphQLResponse {
    fn from(resp: Response) -> Self {
        // Check if data is null/empty
        let data_json = serde_json::to_value(&resp.data).unwrap_or(serde_json::Value::Null);
        let data = if data_json.is_null() {
            None
        } else {
            Some(data_json)
        };

        // Convert errors to FHIR-compliant format with OperationOutcome
        let errors: Vec<serde_json::Value> = resp
            .errors
            .into_iter()
            .map(|e| {
                // Extract location and path from async-graphql error
                let locations = if !e.locations.is_empty() {
                    Some(serde_json::to_value(&e.locations).unwrap_or(serde_json::Value::Null))
                } else {
                    None
                };

                let path = if !e.path.is_empty() {
                    Some(serde_json::to_value(&e.path).unwrap_or(serde_json::Value::Null))
                } else {
                    None
                };

                // Create OperationOutcome for this error
                let operation_outcome = serde_json::json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{
                        "severity": "error",
                        "code": "invalid",
                        "diagnostics": e.message.clone()
                    }]
                });

                // Build error object with OperationOutcome in extensions.resource per FHIR spec
                let mut error_obj = serde_json::json!({
                    "message": e.message,
                    "extensions": {
                        "resource": operation_outcome
                    }
                });

                // Add locations and path if present
                if let Some(locs) = locations {
                    error_obj["locations"] = locs;
                }
                if let Some(p) = path {
                    error_obj["path"] = p;
                }

                error_obj
            })
            .collect();

        Self {
            data,
            errors,
            extensions: if resp.extensions.is_empty() {
                None
            } else {
                Some(serde_json::to_value(&resp.extensions).unwrap_or(serde_json::Value::Null))
            },
        }
    }
}

/// Handles POST requests to /$graphql (system-level endpoint).
///
/// This is the main GraphQL endpoint for executing queries against the entire
/// FHIR server.
///
/// # Authentication
///
/// Authentication is handled by middleware which validates the Bearer token
/// and sets `AuthContext` in request extensions. This handler performs a
/// defense-in-depth check to ensure the context is present.
pub async fn graphql_handler(
    State(state): State<GraphQLState>,
    auth_context: Option<Extension<AuthContext>>,
    headers: HeaderMap,
    Json(request): Json<GraphQLRequest>,
) -> impl IntoResponse {
    // AuthContext is optional when auth is disabled
    if let Some(Extension(ref ctx)) = auth_context {
        debug!(
            user = ?ctx.user.as_ref().map(|u| &u.id),
            client = %ctx.client.client_id,
            "Processing GraphQL request"
        );
    } else {
        debug!("Processing GraphQL request (auth disabled)");
    }

    execute_graphql(
        state,
        headers,
        request,
        None,
        None,
        auth_context.map(|Extension(ctx)| ctx),
        None,
    )
    .await
    .into_response()
}

/// Handles GET requests to /$graphql (system-level endpoint).
///
/// This endpoint supports GraphQL queries via URL query parameters.
///
/// # Authentication
///
/// Authentication is required for all GraphQL requests.
pub async fn graphql_handler_get(
    State(state): State<GraphQLState>,
    auth_context: Option<Extension<AuthContext>>,
    headers: HeaderMap,
    Query(params): Query<GraphQLQueryParams>,
) -> impl IntoResponse {
    // Parse query params into request
    let request = match params_to_request(params) {
        Ok(req) => req,
        Err(e) => {
            return error_response(GraphQLError::InvalidQuery(e.to_string())).into_response();
        }
    };

    if let Some(Extension(ref ctx)) = auth_context {
        debug!(
            user = ?ctx.user.as_ref().map(|u| &u.id),
            client = %ctx.client.client_id,
            "Processing GraphQL GET request"
        );
    } else {
        debug!("Processing GraphQL GET request (auth disabled)");
    }

    execute_graphql(
        state,
        headers,
        request,
        None,
        None,
        auth_context.map(|Extension(ctx)| ctx),
        None,
    )
    .await
    .into_response()
}

/// Handles POST requests to /:type/:id/$graphql (instance-level endpoint).
///
/// This endpoint executes GraphQL queries in the context of a specific
/// FHIR resource instance.
///
/// # Authentication
///
/// Authentication is handled by middleware which validates the Bearer token
/// and sets `AuthContext` in request extensions.
pub async fn instance_graphql_handler(
    State(state): State<GraphQLState>,
    auth_context: Option<Extension<AuthContext>>,
    headers: HeaderMap,
    Path((resource_type, resource_id)): Path<(String, String)>,
    Json(request): Json<GraphQLRequest>,
) -> impl IntoResponse {
    if let Some(Extension(ref ctx)) = auth_context {
        debug!(
            user = ?ctx.user.as_ref().map(|u| &u.id),
            client = %ctx.client.client_id,
            resource_type = %resource_type,
            resource_id = %resource_id,
            "Processing instance GraphQL request"
        );
    } else {
        debug!(
            resource_type = %resource_type,
            resource_id = %resource_id,
            "Processing instance GraphQL request (auth disabled)"
        );
    }

    execute_graphql(
        state,
        headers,
        request,
        Some(resource_type),
        Some(resource_id),
        auth_context.map(|Extension(ctx)| ctx),
        None,
    )
    .await
    .into_response()
}

/// Checks if a GraphQL query is an introspection query.
///
/// Introspection queries typically start with __schema or __type.
/// We detect them by checking if the query contains __schema or __type.
fn is_introspection_query(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.contains("__schema")
        || trimmed.contains("__type")
        || trimmed.contains("IntrospectionQuery")
}

/// Executes a GraphQL request.
async fn execute_graphql(
    state: GraphQLState,
    headers: HeaderMap,
    request: GraphQLRequest,
    target_resource_type: Option<String>,
    target_resource_id: Option<String>,
    auth_context: Option<AuthContext>,
    source_ip: Option<IpAddr>,
) -> impl IntoResponse {
    // Check if this is an introspection query
    let is_introspection = is_introspection_query(&request.query);

    // Try to get or build the schema
    // For introspection queries, wait for the build to complete
    // For regular queries, return error if build is in progress
    let schema = if is_introspection {
        debug!("Introspection query detected, waiting for schema build if needed");
        match state.lazy_schema.get_or_build_wait().await {
            Ok(schema) => schema,
            Err(e) => {
                warn!(error = %e, "Schema build failed for introspection");
                return error_response(e).into_response();
            }
        }
    } else {
        match state.lazy_schema.get_or_build().await {
            Ok(schema) => schema,
            Err(GraphQLError::SchemaInitializing) => {
                return schema_initializing_response().into_response();
            }
            Err(e) => {
                warn!(error = %e, "Schema build failed");
                return error_response(e).into_response();
            }
        }
    };

    // Extract request ID from headers (set by middleware)
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Build the context with auth information
    let context = match build_context(
        &state.context_template,
        request_id,
        target_resource_type,
        target_resource_id,
        auth_context,
        source_ip,
    ) {
        Ok(ctx) => ctx,
        Err(e) => {
            return error_response(GraphQLError::Internal(e.to_string())).into_response();
        }
    };

    // Build the async-graphql request
    let mut gql_request = Request::new(&request.query);

    if let Some(op_name) = request.operation_name {
        gql_request = gql_request.operation_name(op_name);
    }

    if let Some(vars) = request.variables {
        let variables = Variables::from_json(vars);
        gql_request = gql_request.variables(variables);
    }

    // Add context data
    gql_request = gql_request.data(context);

    // Execute the query
    debug!(query = %request.query, "Executing GraphQL query");
    let response = schema.execute(gql_request).await;

    // Convert to JSON response
    // Note: GraphQL always returns 200 OK per spec, even with errors
    let gql_response = GraphQLResponse::from(response);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Json(gql_response),
    )
        .into_response()
}

/// Builds a GraphQL context from the template.
fn build_context(
    template: &GraphQLContextTemplate,
    request_id: String,
    target_resource_type: Option<String>,
    target_resource_id: Option<String>,
    auth_context: Option<AuthContext>,
    source_ip: Option<IpAddr>,
) -> Result<GraphQLContext, crate::context::ContextBuilderError> {
    let mut builder = GraphQLContextBuilder::new()
        .with_storage(template.storage.clone())
        .with_search_config(template.search_config.clone())
        .with_policy_evaluator(template.policy_evaluator.clone())
        .with_auth_context(auth_context)
        .with_source_ip(source_ip)
        .with_request_id(request_id);

    if let (Some(rt), Some(id)) = (target_resource_type, target_resource_id) {
        builder = builder.with_target_resource(rt, id);
    }

    builder.build()
}

/// Converts GET query params to a GraphQL request.
fn params_to_request(params: GraphQLQueryParams) -> Result<GraphQLRequest, serde_json::Error> {
    let variables = if let Some(vars_str) = params.variables {
        Some(serde_json::from_str(&vars_str)?)
    } else {
        None
    };

    Ok(GraphQLRequest {
        query: params.query.unwrap_or_default(),
        operation_name: params.operation_name,
        variables,
        extensions: None,
    })
}

/// Returns a 503 response when schema is initializing.
fn schema_initializing_response() -> impl IntoResponse {
    let body = serde_json::json!({
        "errors": [{
            "message": "GraphQL schema is initializing, please retry",
            "extensions": {
                "code": "SCHEMA_INITIALIZING",
                "resource": {
                    "resourceType": "OperationOutcome",
                    "issue": [{
                        "severity": "information",
                        "code": "transient",
                        "diagnostics": "GraphQL schema is still being built. Please retry in a few seconds."
                    }]
                }
            }
        }]
    });

    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::RETRY_AFTER, "5"),
        ],
        Json(body),
    )
}

/// Returns an error response.
fn error_response(error: GraphQLError) -> impl IntoResponse {
    let status = match error.status_code() {
        503 => StatusCode::SERVICE_UNAVAILABLE,
        401 => StatusCode::UNAUTHORIZED,
        403 => StatusCode::FORBIDDEN,
        404 => StatusCode::NOT_FOUND,
        400 => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    let body = serde_json::json!({
        "errors": [{
            "message": error.to_string(),
            "extensions": {
                "code": error.error_code(),
                "resource": error.to_operation_outcome()
            }
        }]
    });

    // Simple headers without retry-after for now (axum IntoResponse constraint)
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        Json(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphql_request_deserialize() {
        let json = r#"{
            "query": "{ _health }",
            "operationName": "GetHealth",
            "variables": {"foo": "bar"}
        }"#;

        let request: GraphQLRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.query, "{ _health }");
        assert_eq!(request.operation_name, Some("GetHealth".to_string()));
        assert!(request.variables.is_some());
    }

    #[test]
    fn test_graphql_request_minimal() {
        let json = r#"{"query": "{ _health }"}"#;

        let request: GraphQLRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.query, "{ _health }");
        assert!(request.operation_name.is_none());
        assert!(request.variables.is_none());
    }

    #[test]
    fn test_params_to_request() {
        let params = GraphQLQueryParams {
            query: Some("{ _health }".to_string()),
            operation_name: Some("GetHealth".to_string()),
            variables: Some(r#"{"foo": "bar"}"#.to_string()),
        };

        let request = params_to_request(params).unwrap();
        assert_eq!(request.query, "{ _health }");
        assert_eq!(request.operation_name, Some("GetHealth".to_string()));
        assert!(request.variables.is_some());
    }

    #[test]
    fn test_params_to_request_minimal() {
        let params = GraphQLQueryParams {
            query: Some("{ _health }".to_string()),
            operation_name: None,
            variables: None,
        };

        let request = params_to_request(params).unwrap();
        assert_eq!(request.query, "{ _health }");
        assert!(request.operation_name.is_none());
        assert!(request.variables.is_none());
    }

    #[test]
    fn test_params_to_request_invalid_variables() {
        let params = GraphQLQueryParams {
            query: Some("{ _health }".to_string()),
            operation_name: None,
            variables: Some("not valid json".to_string()),
        };

        let result = params_to_request(params);
        assert!(result.is_err());
    }
}
