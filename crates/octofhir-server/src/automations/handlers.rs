//! Automation API handlers.
//!
//! This module provides CRUD handlers for managing automations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use octofhir_api::ApiError;
use octofhir_auth::middleware::AdminAuth;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use super::executor::AutomationExecutor;
use super::storage::{AutomationStorage, PostgresAutomationStorage};
use super::types::{
    Automation, AutomationTrigger, CreateAutomation, CreateAutomationTrigger, UpdateAutomation,
};

use super::types::AutomationEvent;
use otter_runtime::{is_typescript, transpile_typescript};
use sqlx_postgres::PgPool;

// =============================================================================
// State
// =============================================================================

/// State for automation handlers.
#[derive(Clone)]
pub struct AutomationState {
    pub automation_storage: Arc<dyn AutomationStorage>,
    pub executor: Arc<AutomationExecutor>,
}

impl AutomationState {
    /// Create a new AutomationState from PostgreSQL pool and executor.
    pub fn new(pool: PgPool, executor: Arc<AutomationExecutor>) -> Self {
        Self {
            automation_storage: Arc::new(PostgresAutomationStorage::new(pool)),
            executor,
        }
    }
}

// =============================================================================
// Request/Response Types
// =============================================================================

/// Search parameters for automations.
#[derive(Debug, Deserialize)]
pub struct AutomationSearchParams {
    /// Filter by status
    pub status: Option<String>,

    /// Filter by name (partial match)
    pub name: Option<String>,

    /// Maximum results to return
    #[serde(rename = "_count")]
    pub count: Option<i64>,

    /// Number of results to skip
    #[serde(rename = "_offset")]
    pub offset: Option<i64>,
}

/// Request to execute an automation manually.
#[derive(Debug, Deserialize)]
pub struct ExecuteAutomationRequest {
    /// Optional resource to pass as event context
    pub resource: Option<serde_json::Value>,

    /// Optional event type (defaults to "manual")
    pub event_type: Option<String>,
}

/// Response wrapper for automation with triggers.
#[derive(Debug, serde::Serialize)]
pub struct AutomationWithTriggers {
    #[serde(flatten)]
    pub automation: Automation,
    pub triggers: Vec<AutomationTrigger>,
}

/// Bundle response for lists.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationBundle<T> {
    pub resource_type: String,
    #[serde(rename = "type")]
    pub bundle_type: String,
    pub total: usize,
    pub entry: Vec<AutomationBundleEntry<T>>,
}

#[derive(Debug, serde::Serialize)]
pub struct AutomationBundleEntry<T> {
    pub resource: T,
}

impl<T> AutomationBundle<T> {
    pub fn searchset(resources: Vec<T>) -> Self {
        let total = resources.len();
        Self {
            resource_type: "Bundle".to_string(),
            bundle_type: "searchset".to_string(),
            total,
            entry: resources
                .into_iter()
                .map(|r| AutomationBundleEntry { resource: r })
                .collect(),
        }
    }
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /api/automations - List all automations.
pub async fn list_automations(
    State(state): State<AutomationState>,
    Query(params): Query<AutomationSearchParams>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let automations = state
        .automation_storage
        .list_automations()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Apply filters
    let filtered: Vec<Automation> = automations
        .into_iter()
        .filter(|automation| {
            // Filter by status
            if let Some(ref status_filter) = params.status {
                if automation.status.as_str() != status_filter {
                    return false;
                }
            }

            // Filter by name
            if let Some(ref name_filter) = params.name {
                if !automation
                    .name
                    .to_lowercase()
                    .contains(&name_filter.to_lowercase())
                {
                    return false;
                }
            }

            true
        })
        .collect();

    // Apply pagination
    let offset = params.offset.unwrap_or(0) as usize;
    let count = params.count.unwrap_or(100) as usize;
    let paginated: Vec<Automation> = filtered.into_iter().skip(offset).take(count).collect();

    let bundle = AutomationBundle::searchset(paginated);

    Ok(Json(bundle))
}

/// GET /api/automations/:id - Get a single automation.
pub async fn get_automation(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let automation = state
        .automation_storage
        .get_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    // Get triggers
    let triggers = state
        .automation_storage
        .get_triggers(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let response = AutomationWithTriggers {
        automation,
        triggers,
    };

    Ok(Json(response))
}

/// POST /api/automations - Create a new automation.
pub async fn create_automation(
    State(state): State<AutomationState>,
    _admin: AdminAuth,
    Json(create): Json<CreateAutomation>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate
    if create.name.is_empty() {
        return Err(ApiError::bad_request("Automation name is required"));
    }
    if create.source_code.is_empty() {
        return Err(ApiError::bad_request("Automation source code is required"));
    }

    let automation = state
        .automation_storage
        .create_automation(create)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get triggers
    let triggers = state
        .automation_storage
        .get_triggers(automation.id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %automation.id, name = %automation.name, "Created automation");

    let response = AutomationWithTriggers {
        automation,
        triggers,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// PUT /api/automations/:id - Update an automation.
pub async fn update_automation(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(update): Json<UpdateAutomation>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let automation = state
        .automation_storage
        .update_automation(uuid, update)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    // Get triggers
    let triggers = state
        .automation_storage
        .get_triggers(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %automation.id, name = %automation.name, "Updated automation");

    let response = AutomationWithTriggers {
        automation,
        triggers,
    };

    Ok(Json(response))
}

/// DELETE /api/automations/:id - Delete an automation.
pub async fn delete_automation(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let deleted = state
        .automation_storage
        .delete_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !deleted {
        return Err(ApiError::not_found(format!("Automation/{}", id)));
    }

    tracing::info!(id = %id, "Deleted automation");

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/automations/:id/deploy - Deploy (activate) an automation.
///
/// This endpoint:
/// 1. Validates the automation's source code
/// 2. Transpiles TypeScript to JavaScript (if needed)
/// 3. Stores the compiled code
/// 4. Sets the automation status to active
pub async fn deploy_automation(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Get the automation
    let automation = state
        .automation_storage
        .get_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    // Transpile TypeScript to JavaScript if needed
    let compiled_code = if is_typescript(&automation.source_code) {
        tracing::debug!(automation_id = %automation.id, "Transpiling TypeScript to JavaScript");
        match transpile_typescript(&automation.source_code) {
            Ok(result) => {
                tracing::info!(
                    automation_id = %automation.id,
                    source_len = automation.source_code.len(),
                    compiled_len = result.code.len(),
                    "TypeScript transpilation successful"
                );
                result.code
            }
            Err(e) => {
                tracing::warn!(automation_id = %automation.id, error = %e, "TypeScript transpilation failed");
                return Err(ApiError::bad_request(format!(
                    "TypeScript compilation failed: {}",
                    e
                )));
            }
        }
    } else {
        // Plain JavaScript - use source as compiled
        automation.source_code.clone()
    };

    // Deploy the automation with compiled code
    let deployed_automation = state
        .automation_storage
        .deploy_automation(uuid, compiled_code)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    tracing::info!(id = %deployed_automation.id, name = %deployed_automation.name, "Deployed automation");

    Ok(Json(deployed_automation))
}

/// POST /api/automations/:id/execute - Execute an automation manually.
pub async fn execute_automation(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(request): Json<ExecuteAutomationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let automation = state
        .automation_storage
        .get_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    // Create event for manual execution
    let event = AutomationEvent {
        event_type: request.event_type.unwrap_or_else(|| "manual".to_string()),
        resource: request.resource.unwrap_or(serde_json::json!({})),
        previous: None,
        timestamp: time::OffsetDateTime::now_utc().to_string(),
    };

    // Execute the automation
    let result = state.executor.execute(&automation, None, event).await;

    let response = serde_json::json!({
        "executionId": result.execution_id.to_string(),
        "success": result.success,
        "output": result.output,
        "error": result.error,
        "durationMs": result.duration.as_millis() as u64,
    });

    if result.success {
        Ok(Json(response))
    } else {
        // Return 200 with error details in body (automation execution failed, not API error)
        Ok(Json(response))
    }
}

/// GET /api/automations/:id/logs - Get execution logs for an automation.
pub async fn get_automation_logs(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    Query(params): Query<AutomationSearchParams>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Verify automation exists
    let _ = state
        .automation_storage
        .get_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    let limit = params.count.unwrap_or(50);

    let executions = state
        .automation_storage
        .get_executions(uuid, limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let bundle = AutomationBundle::searchset(executions);

    Ok(Json(bundle))
}

/// POST /api/automations/:id/triggers - Add a trigger to an automation.
pub async fn add_trigger(
    State(state): State<AutomationState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(trigger): Json<CreateAutomationTrigger>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Verify automation exists
    let _ = state
        .automation_storage
        .get_automation(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Automation/{}", id)))?;

    let created_trigger = state
        .automation_storage
        .create_trigger(uuid, trigger)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(automation_id = %id, trigger_id = %created_trigger.id, "Added trigger to automation");

    Ok((StatusCode::CREATED, Json(created_trigger)))
}

/// DELETE /api/automations/:automation_id/triggers/:trigger_id - Remove a trigger from an automation.
pub async fn delete_trigger(
    State(state): State<AutomationState>,
    Path((automation_id, trigger_id)): Path<(String, String)>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let _automation_uuid = Uuid::parse_str(&automation_id)
        .map_err(|_| ApiError::bad_request("Invalid automation UUID format"))?;
    let trigger_uuid = Uuid::parse_str(&trigger_id)
        .map_err(|_| ApiError::bad_request("Invalid trigger UUID format"))?;

    let deleted = state
        .automation_storage
        .delete_trigger(trigger_uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !deleted {
        return Err(ApiError::not_found(format!("Trigger/{}", trigger_id)));
    }

    tracing::info!(automation_id = %automation_id, trigger_id = %trigger_id, "Deleted trigger from automation");

    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Routes
// =============================================================================

use axum::Router;
use axum::extract::FromRef;
use axum::routing::{delete, get, post};
use octofhir_auth::middleware::AuthState;

/// Creates the automation routes.
///
/// All routes require admin authentication via the `AdminAuth` extractor.
pub fn automation_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    AuthState: FromRef<S>,
    AutomationState: FromRef<S>,
{
    Router::new()
        // Automation CRUD
        .route("/automations", get(list_automations).post(create_automation))
        .route(
            "/automations/{id}",
            get(get_automation)
                .put(update_automation)
                .delete(delete_automation),
        )
        // Automation actions
        .route("/automations/{id}/deploy", post(deploy_automation))
        .route("/automations/{id}/execute", post(execute_automation))
        .route("/automations/{id}/logs", get(get_automation_logs))
        // Triggers
        .route("/automations/{id}/triggers", post(add_trigger))
        .route(
            "/automations/{automation_id}/triggers/{trigger_id}",
            delete(delete_trigger),
        )
}
