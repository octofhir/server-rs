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
    ParamsSearchConfig, SqlValue, UnknownParamHandling,
    build_native_ir_query_from_params_with_config, parse_query_string,
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
pub struct ParamValue {
    /// `$1`, `$2`, …
    pub placeholder: String,
    /// SqlValue variant kind (Text, Integer, …).
    pub kind: String,
    /// The actual bind value (admin tool — values are shown so the query is runnable).
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct ParsedParam {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExplainResponse {
    pub resource_type: String,
    /// Parsed search IR (predicates, index strategy).
    pub parsed_ir: Option<SearchDebugPlan>,
    /// Every parsed query parameter (including _has / chained / _include), so the full
    /// query intent is visible even where the debug plan only models simple predicates.
    pub parsed_params: Vec<ParsedParam>,
    /// Generated SQL with `$1, $2, ...` placeholders.
    pub sql: String,
    /// The same SQL with bind values inlined as literals — copy/paste runnable in psql.
    pub runnable_sql: String,
    /// Bind values (kind + actual value).
    pub params: Vec<ParamValue>,
    /// Search parameters that were not recognised for this resource type.
    pub unknown_params: Vec<UnknownParam>,
    /// Whether `EXPLAIN (ANALYZE)` was run (the query was executed).
    pub analyzed: bool,
    /// The raw Postgres `EXPLAIN (FORMAT JSON)` output (drives the graph).
    pub explain_plan: serde_json::Value,
    /// The classic indented `EXPLAIN (FORMAT TEXT)` plan — easier to read.
    pub explain_text: String,
}

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": message.into() })))
}

fn describe_value(v: &SqlValue) -> (&'static str, String) {
    match v {
        SqlValue::Text(s) => ("Text", s.clone()),
        SqlValue::Integer(i) => ("Integer", i.to_string()),
        SqlValue::Float(f) => ("Float", f.to_string()),
        SqlValue::Boolean(b) => ("Boolean", b.to_string()),
        SqlValue::Json(s) => ("Json", s.clone()),
        SqlValue::Timestamp(s) => ("Timestamp", s.clone()),
        SqlValue::Null => ("Null", "NULL".to_string()),
    }
}

/// SQL literal for a bind value, so the query can be inlined and run directly.
fn sql_literal(v: &SqlValue) -> String {
    match v {
        SqlValue::Integer(i) => i.to_string(),
        SqlValue::Float(f) => f.to_string(),
        SqlValue::Boolean(b) => b.to_string(),
        SqlValue::Null => "NULL".to_string(),
        SqlValue::Json(s) => format!("'{}'::jsonb", s.replace('\'', "''")),
        SqlValue::Timestamp(s) => format!("'{}'::timestamptz", s.replace('\'', "''")),
        SqlValue::Text(s) => format!("'{}'", s.replace('\'', "''")),
    }
}

/// Inline `$1..$n` placeholders with literals (highest index first so `$10` beats `$1`).
fn inline_sql(sql: &str, params: &[SqlValue]) -> String {
    let mut out = sql.to_string();
    for (i, v) in params.iter().enumerate().rev() {
        out = out.replace(&format!("${}", i + 1), &sql_literal(v));
    }
    out
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

    let explain_text = octofhir_db_postgres::queries::search::explain_built_search_query_text(
        state.db_pool.as_ref(),
        &built,
        req.analyze,
    )
    .await
    .unwrap_or_default();

    let param_values = built
        .params
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let (kind, value) = describe_value(v);
            ParamValue {
                placeholder: format!("${}", i + 1),
                kind: kind.to_string(),
                value,
            }
        })
        .collect();

    let runnable_sql = inline_sql(&built.sql, &built.params);

    // Echo every parsed parameter (incl. _has / chained / _include) for a full IR view.
    let mut parsed_params: Vec<ParsedParam> = params
        .parameters
        .iter()
        .map(|(name, values)| ParsedParam {
            name: name.clone(),
            values: values.clone(),
        })
        .collect();
    parsed_params.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(ExplainResponse {
        resource_type: req.resource_type,
        parsed_ir: converted.debug_plan,
        parsed_params,
        sql: built.sql,
        runnable_sql,
        params: param_values,
        unknown_params,
        analyzed: req.analyze,
        explain_plan,
        explain_text,
    }))
}
