//! `/api/metrics/*`: the console's dashboards query Prometheus through these, admin only.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use octofhir_auth::middleware::AdminAuth;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::client::{MetricInfo, ParseError, PrometheusClient, Target};
use super::dashboards::{self, Dashboard};
use super::error::MetricsError;
use super::explain::{self, ExplainNode};
use super::grafana::{
    self, GrafanaClient, GrafanaDatasource, GrafanaFolder, Imported, PushedDashboard,
};
use super::promql::Variables;
use crate::server::AppState;

fn client(state: &AppState) -> Result<&PrometheusClient, MetricsError> {
    state.prometheus.as_ref().ok_or(MetricsError::NotConfigured)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    configured: bool,
    selector: Option<String>,
    scrape_interval_secs: Option<u64>,
    reachable: bool,
    error: Option<String>,
    targets: Vec<Target>,
}

/// `GET /api/metrics/status`: whether Prometheus is configured and reachable, and which
/// instances it scrapes.
pub async fn status(State(state): State<AppState>, _admin: AdminAuth) -> Json<StatusResponse> {
    let Some(client) = state.prometheus.as_ref() else {
        return Json(StatusResponse {
            configured: false,
            selector: None,
            scrape_interval_secs: None,
            reachable: false,
            error: None,
            targets: Vec::new(),
        });
    };
    let (reachable, error, targets) = match client.targets().await {
        Ok(targets) => (true, None, targets),
        Err(error) => (false, Some(error.to_string()), Vec::new()),
    };
    Json(StatusResponse {
        configured: true,
        selector: Some(client.selector().to_string()),
        scrape_interval_secs: Some(client.scrape_interval().as_secs()),
        reachable,
        error,
        targets,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeRequest {
    query: String,
    /// Unix seconds.
    start: f64,
    end: f64,
    /// Requested step in seconds; raised to the scrape interval and to fit `max_points`.
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    variables: BTreeMap<String, String>,
}

/// `POST /api/metrics/query_range`: a scoped range query. The response carries the step actually
/// used, so the chart's axis matches the data.
pub async fn query_range(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(request): Json<RangeRequest>,
) -> Result<Json<Value>, MetricsError> {
    let client = client(&state)?;
    let window = client.window(request.start, request.end, request.step)?;
    let step = window.step.ceil() as u64;
    let range = (window.end - window.start).ceil() as u64;
    let variables =
        Variables::new(client.rate_window(window.step), step, range).with_user(&request.variables);
    let query = variables.expand(&request.query).query;
    let data = client.query_range(&query, window).await?;
    Ok(Json(json!({ "window": window, "data": data })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstantRequest {
    query: String,
    /// Unix seconds; now when absent.
    #[serde(default)]
    time: Option<f64>,
    /// Seconds `$__range` stands for (the dashboard's time range).
    #[serde(default)]
    range: Option<f64>,
    #[serde(default)]
    variables: BTreeMap<String, String>,
}

/// `POST /api/metrics/query`: a scoped instant query, for stat and table panels.
pub async fn query(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(request): Json<InstantRequest>,
) -> Result<Json<Value>, MetricsError> {
    let client = client(&state)?;
    let scrape = client.scrape_interval().as_secs();
    let range = request
        .range
        .filter(|r| r.is_finite() && *r > 0.0)
        .map(|r| r.ceil() as u64)
        .unwrap_or(scrape * 4);
    let variables = Variables::new(client.rate_window(scrape as f64), scrape, range)
        .with_user(&request.variables);
    let query = variables.expand(&request.query).query;
    let data = client.query(&query, request.time).await?;
    Ok(Json(json!({ "data": data })))
}

/// `POST /api/metrics/explain`: the query's syntax tree with every sub-expression evaluated as
/// an instant query (series count and a few samples), to see where a query gains or loses data.
pub async fn explain_query(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(request): Json<InstantRequest>,
) -> Result<Json<ExplainNode>, MetricsError> {
    let client = client(&state)?;
    let scrape = client.scrape_interval().as_secs();
    let range = request
        .range
        .filter(|r| r.is_finite() && *r > 0.0)
        .map(|r| r.ceil() as u64)
        .unwrap_or(scrape * 4);
    let variables = Variables::new(client.rate_window(scrape as f64), scrape, range)
        .with_user(&request.variables);
    let query = variables.expand(&request.query).query;
    let mut tree = explain::tree(&query)?;
    explain::evaluate(client, &mut tree, request.time).await;
    Ok(Json(tree))
}

/// `GET /api/metrics/metrics`: metric names this deployment exports, with type and help.
pub async fn metrics(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> Result<Json<Vec<MetricInfo>>, MetricsError> {
    Ok(Json(client(&state)?.metrics().await?))
}

#[derive(Deserialize)]
pub struct MetricFilter {
    #[serde(default)]
    metric: Option<String>,
}

/// `GET /api/metrics/labels?metric=`: label names of this deployment's series.
pub async fn labels(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Query(filter): Query<MetricFilter>,
) -> Result<Json<Vec<String>>, MetricsError> {
    let metric = filter.metric.as_deref().filter(|m| !m.is_empty());
    Ok(Json(client(&state)?.labels(metric).await?))
}

/// `GET /api/metrics/label/{name}/values?metric=`: values of one label.
pub async fn label_values(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(name): Path<String>,
    Query(filter): Query<MetricFilter>,
) -> Result<Json<Vec<String>>, MetricsError> {
    let metric = filter.metric.as_deref().filter(|m| !m.is_empty());
    Ok(Json(client(&state)?.label_values(&name, metric).await?))
}

#[derive(Deserialize)]
pub struct ParseRequest {
    query: String,
}

#[derive(Serialize)]
pub struct ParseResponse {
    ok: bool,
    error: Option<ParseError>,
}

/// `POST /api/metrics/parse`: syntax check for the editor, with the error position as written
/// (before variables were substituted).
pub async fn parse(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(request): Json<ParseRequest>,
) -> Result<Json<ParseResponse>, MetricsError> {
    let client = client(&state)?;
    let scrape = client.scrape_interval().as_secs();
    let expanded = Variables::new(scrape * 4, scrape, 3600).expand(&request.query);
    let error = client.check(&expanded.query).await?.map(|mut error| {
        if let (Some(line), Some(column)) = (error.line, error.column) {
            let (line, column) = expanded.original_position(line, column);
            error.line = Some(line);
            error.column = Some(column);
        }
        error
    });
    Ok(Json(ParseResponse {
        ok: error.is_none(),
        error,
    }))
}

/// `GET /api/dashboards/builtin`: the dashboards shipped with the server.
pub async fn builtin_dashboards(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> Result<Json<Vec<Dashboard>>, MetricsError> {
    client(&state)?;
    Ok(Json(dashboards::builtin()))
}

/// `POST /api/dashboards/grafana/export`: a dashboard as Grafana JSON for import, with its
/// queries scoped to the selector and a datasource input for Grafana to ask about.
pub async fn export_grafana(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(dashboard): Json<Dashboard>,
) -> Result<Json<Value>, MetricsError> {
    let client = client(&state)?;
    dashboard.validate().map_err(MetricsError::BadRequest)?;
    Ok(Json(grafana::export(
        &dashboard,
        client.matchers(),
        client.selector(),
        None,
    )?))
}

/// `POST /api/dashboards/import`: a Grafana dashboard JSON (or this console's own) as a
/// dashboard to save, with what could not be carried over. Nothing is stored here.
pub async fn import_dashboard(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(json): Json<Value>,
) -> Result<Json<Imported>, MetricsError> {
    client(&state)?;
    Ok(Json(grafana::import(&json)?))
}

/// Where to reach Grafana. Deliberately not `Debug`: the token must not reach logs.
#[derive(Deserialize)]
pub struct GrafanaTarget {
    url: String,
    token: String,
}

impl GrafanaTarget {
    fn connect(&self, state: &AppState) -> Result<GrafanaClient, MetricsError> {
        GrafanaClient::new(&self.url, &self.token, &state.subscription_state.egress)
    }
}

/// `POST /api/grafana/datasources`: Prometheus datasources of the named Grafana; also checks the
/// token works.
pub async fn grafana_datasources(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(target): Json<GrafanaTarget>,
) -> Result<Json<Vec<GrafanaDatasource>>, MetricsError> {
    Ok(Json(target.connect(&state)?.datasources().await?))
}

/// `POST /api/grafana/folders`: folders of the named Grafana.
pub async fn grafana_folders(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(target): Json<GrafanaTarget>,
) -> Result<Json<Vec<GrafanaFolder>>, MetricsError> {
    Ok(Json(target.connect(&state)?.folders().await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    #[serde(flatten)]
    target: GrafanaTarget,
    datasource_uid: String,
    #[serde(default)]
    folder_uid: Option<String>,
    dashboards: Vec<Dashboard>,
}

/// Most dashboards one push may carry.
const MAX_PUSH: usize = 20;

/// `POST /api/grafana/push`: creates or updates the dashboards in Grafana (same uid each time).
pub async fn grafana_push(
    State(state): State<AppState>,
    admin: AdminAuth,
    Json(request): Json<PushRequest>,
) -> Result<Json<Vec<PushedDashboard>>, MetricsError> {
    let prometheus = client(&state)?;
    if request.dashboards.is_empty() || request.dashboards.len() > MAX_PUSH {
        return Err(MetricsError::bad_request(format!(
            "push 1 to {MAX_PUSH} dashboards"
        )));
    }
    if request.datasource_uid.trim().is_empty() {
        return Err(MetricsError::bad_request("choose a Grafana datasource"));
    }
    let grafana = request.target.connect(&state)?;
    let mut pushed = Vec::with_capacity(request.dashboards.len());
    for dashboard in &request.dashboards {
        dashboard.validate().map_err(MetricsError::BadRequest)?;
        let json = grafana::export(
            dashboard,
            prometheus.matchers(),
            prometheus.selector(),
            Some(request.datasource_uid.trim()),
        )?;
        pushed.push(grafana.push(json, request.folder_uid.as_deref()).await?);
    }
    tracing::info!(
        user = %admin.username,
        dashboards = pushed.len(),
        "Pushed dashboards to Grafana"
    );
    Ok(Json(pushed))
}
