//! `POST /api/console/explain` — "Explain this query" for the REST console.
//!
//! Given a FHIR search (resource type + query string), returns the parsed search IR,
//! the generated SQL (with redacted bind values), and the Postgres `EXPLAIN` plan.
//! This is a pure-reuse surface over `octofhir-search` + `octofhir-db-postgres`; it
//! builds the same query the real search path would and asks Postgres to plan it.
//!
//! By default `analyze = false`, so we only plan the query (no execution, no data
//! touched). `analyze = true` runs `EXPLAIN (ANALYZE)` and therefore executes the query.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use octofhir_search::ir::SearchDebugPlan;
use octofhir_search::{
    ParamsSearchConfig, UnknownParamHandling, build_native_ir_query_from_params_with_config,
    parse_query_string,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::server::AppState;

/// Postgres schema FHIR resource tables live in.
const SEARCH_SCHEMA: &str = "public";

#[derive(Debug, Deserialize)]
pub struct ExplainRequest {
    /// FHIR resource type, e.g. "Patient".
    pub resource_type: String,
    /// Raw query string, e.g. "name=smith&_count=10" (no leading `?`).
    #[serde(default)]
    pub query: String,
    /// Run `EXPLAIN (ANALYZE)` (executes the query) instead of plan-only.
    #[serde(default)]
    pub analyze: bool,
}

#[derive(Debug, Serialize)]
pub struct UnknownParam {
    pub name: String,
    pub modifier: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExplainResponse {
    pub resource_type: String,
    /// Parsed search IR (predicates, index strategy). Safe — carries no bind values.
    pub parsed_ir: Option<SearchDebugPlan>,
    /// Generated SQL with `$1, $2, ...` placeholders (bind values are not inlined).
    pub sql: String,
    /// Redacted bind values — only the SqlValue variant kind, never the actual value.
    pub params: Vec<String>,
    /// Search parameters that were not recognised for this resource type.
    pub unknown_params: Vec<UnknownParam>,
    /// Whether `EXPLAIN (ANALYZE)` was run (the query was executed).
    pub analyzed: bool,
    /// The raw Postgres `EXPLAIN (FORMAT JSON)` output.
    pub explain_plan: serde_json::Value,
}

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": message.into() })))
}

/// Redact a bind value to its kind only, so no search term / PHI leaves the server.
fn redact_param(value: &str) -> String {
    // Debug repr looks like `Text("smith")` / `Integer(10)` — keep only the kind.
    match value.split_once('(') {
        Some((kind, _)) => kind.to_string(),
        None => value.to_string(),
    }
}

pub async fn explain_search(
    State(state): State<AppState>,
    Json(req): Json<ExplainRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if req.resource_type.trim().is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "resource_type is required"));
    }

    let cfg = state.search_config.config();
    let params = parse_query_string(&req.query, cfg.default_count as u32, cfg.max_count as u32);

    let build_cfg = ParamsSearchConfig {
        unknown_param_handling: UnknownParamHandling::Lenient,
        collect_debug_plan: true,
    };

    let converted = build_native_ir_query_from_params_with_config(
        &req.resource_type,
        &params,
        cfg.registry.as_ref(),
        SEARCH_SCHEMA,
        &build_cfg,
    )
    .map_err(|e| {
        error(
            StatusCode::BAD_REQUEST,
            format!("failed to build query: {e}"),
        )
    })?;

    let unknown_params = converted
        .unknown_params
        .iter()
        .map(|w| UnknownParam {
            name: w.name.clone(),
            modifier: w.modifier.clone(),
        })
        .collect();

    let built = converted
        .builder
        .with_raw_resource(true)
        .build()
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to render SQL: {e}"),
            )
        })?;

    let explain_plan = octofhir_db_postgres::queries::search::explain_built_search_query_json(
        state.db_pool.as_ref(),
        &built,
        req.analyze,
    )
    .await
    .map_err(|e| {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("EXPLAIN failed: {e}"),
        )
    })?;

    let params = built
        .params
        .iter()
        .map(|v| redact_param(&format!("{v:?}")))
        .collect();

    Ok(Json(ExplainResponse {
        resource_type: req.resource_type,
        parsed_ir: converted.debug_plan,
        sql: built.sql,
        params,
        unknown_params,
        analyzed: req.analyze,
        explain_plan,
    }))
}
