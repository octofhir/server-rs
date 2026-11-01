//! Grafana export: a [`Dashboard`] as Grafana dashboard JSON, and pushing it to a Grafana the
//! user names. Grafana is optional; nothing here is configured on the server. The URL and
//! service-account token arrive with each request and are never stored or logged.

use std::sync::Arc;
use std::time::Duration;

use promql_parser::label::Matcher;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::dashboards::{CursorSync, Dashboard, DrawStyle, Panel, PanelType, SeriesColor};
use super::error::MetricsError;
use super::promql;
use crate::egress::EgressPolicy;

/// Grafana's datasource input name; importing the JSON asks which Prometheus it stands for.
const DATASOURCE_INPUT: &str = "DS_PROMETHEUS";
const DATASOURCE_REF: &str = "${DS_PROMETHEUS}";
const SCHEMA_VERSION: u32 = 39;
const GRAFANA_TIMEOUT: Duration = Duration::from_secs(15);

/// Converts `dashboard` to Grafana JSON. `datasource_uid` fixes the datasource (push); without
/// it the JSON declares an input so Grafana's import dialog asks for one (download).
pub fn export(
    dashboard: &Dashboard,
    scope: &[Matcher],
    selector: &str,
    datasource_uid: Option<&str>,
) -> Result<Value, MetricsError> {
    let datasource =
        json!({ "type": "prometheus", "uid": datasource_uid.unwrap_or(DATASOURCE_REF) });
    let panels = dashboard
        .panels
        .iter()
        .enumerate()
        .map(|(index, panel)| export_panel(panel, index as u64 + 1, scope, &datasource))
        .collect::<Result<Vec<_>, _>>()?;
    let variables: Vec<Value> = dashboard
        .variables
        .iter()
        .map(|variable| {
            let series = format!("{}{selector}", variable.metric.as_deref().unwrap_or(""));
            let query = format!("label_values({series}, {})", variable.label_name);
            json!({
                "name": variable.name,
                "label": variable.label.clone().unwrap_or_else(|| variable.name.clone()),
                "type": "query",
                "datasource": datasource,
                "definition": query,
                "query": { "query": query, "refId": "PrometheusVariableQueryEditor-VariableQuery" },
                "refresh": 2,
                "sort": 1,
                "multi": variable.multi,
                "includeAll": variable.include_all,
                "allValue": if variable.include_all { json!(".*") } else { Value::Null },
                "current": {},
                "options": [],
            })
        })
        .collect();

    let mut out = json!({
        "uid": grafana_uid(dashboard),
        "title": dashboard.title,
        "description": dashboard.description,
        "tags": ["octofhir"],
        "timezone": "browser",
        "editable": true,
        "schemaVersion": SCHEMA_VERSION,
        "graphTooltip": match dashboard.cursor_sync {
            CursorSync::Off => 0,
            CursorSync::Crosshair => 1,
            CursorSync::Tooltip => 2,
        },
        "time": { "from": dashboard.time.from, "to": dashboard.time.to },
        "refresh": "30s",
        "templating": { "list": variables },
        "panels": panels,
    });
    if datasource_uid.is_none() {
        out["__inputs"] = json!([{
            "name": DATASOURCE_INPUT,
            "label": "Prometheus",
            "description": "Prometheus that scrapes OctoFHIR",
            "type": "datasource",
            "pluginId": "prometheus",
            "pluginName": "Prometheus",
        }]);
        out["__requires"] = json!([
            { "type": "grafana", "id": "grafana", "name": "Grafana", "version": "10.0.0" },
            { "type": "datasource", "id": "prometheus", "name": "Prometheus", "version": "1.0.0" },
            { "type": "panel", "id": "timeseries", "name": "Time series", "version": "" },
            { "type": "panel", "id": "stat", "name": "Stat", "version": "" },
            { "type": "panel", "id": "table", "name": "Table", "version": "" },
        ]);
    }
    Ok(out)
}

/// Stable uid, so pushing again updates the same Grafana dashboard. Grafana caps uids at 40.
fn grafana_uid(dashboard: &Dashboard) -> String {
    let id = dashboard.id.as_deref().unwrap_or(&dashboard.title);
    let slug: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut uid = format!("octofhir-{slug}");
    uid.truncate(40);
    uid
}

fn export_panel(
    panel: &Panel,
    id: u64,
    scope: &[Matcher],
    datasource: &Value,
) -> Result<Value, MetricsError> {
    let instant = panel.kind == PanelType::Table;
    let targets = panel
        .targets
        .iter()
        .map(|target| {
            let expr = promql::scope_template(&target.expr, scope).map_err(|e| {
                MetricsError::Query(format!(
                    "panel {:?}, query {}: {e}",
                    panel.title, target.ref_id
                ))
            })?;
            Ok(json!({
                "refId": target.ref_id,
                "datasource": datasource,
                "expr": expr,
                "legendFormat": target.legend.clone().unwrap_or_else(|| "__auto".into()),
                "range": !instant,
                "instant": instant,
                "format": if instant { "table" } else { "time_series" },
            }))
        })
        .collect::<Result<Vec<_>, MetricsError>>()?;

    let mut defaults = json!({ "color": { "mode": "palette-classic" } });
    if let Some(unit) = &panel.unit {
        defaults["unit"] = json!(unit);
    }
    if let Some(decimals) = panel.decimals {
        defaults["decimals"] = json!(decimals);
    }
    if let Some(min) = panel.min {
        defaults["min"] = json!(min);
    }
    if let Some(max) = panel.max {
        defaults["max"] = json!(max);
    }
    // Neutral base: only a threshold that is reached colours a value.
    let mut steps = vec![json!({ "color": "text", "value": null })];
    steps.extend(panel.thresholds.iter().map(
        |threshold| json!({ "color": grafana_color(threshold.color), "value": threshold.value }),
    ));
    defaults["thresholds"] = json!({ "mode": "absolute", "steps": steps });

    let mut overrides: Vec<Value> = panel
        .colors
        .iter()
        .map(|(name, color)| {
            json!({
                "matcher": { "id": "byName", "options": name },
                "properties": [{ "id": "color", "value": { "mode": "fixed", "fixedColor": grafana_color(*color) } }],
            })
        })
        .collect();
    // Per-query units: tables name each value column after its query's legend, charts match
    // the query by refId.
    overrides.extend(panel.targets.iter().filter_map(|target| {
        let unit = target.unit.as_ref()?;
        let matcher = if instant {
            json!({ "id": "byName", "options": target.legend.clone().unwrap_or_else(|| target.ref_id.clone()) })
        } else {
            json!({ "id": "byFrameRefID", "options": target.ref_id })
        };
        Some(json!({ "matcher": matcher, "properties": [{ "id": "unit", "value": unit }] }))
    }));

    let (kind, options, transformations) = match panel.kind {
        PanelType::Timeseries => {
            let bars = panel.draw_style == DrawStyle::Bars;
            defaults["custom"] = json!({
                "drawStyle": if bars { "bars" } else { "line" },
                "lineWidth": if bars { 1 } else { 2 },
                "fillOpacity": if bars { 80 } else { 0 },
                "showPoints": "never",
                "stacking": { "mode": if panel.stack { "normal" } else { "none" }, "group": "A" },
            });
            (
                "timeseries",
                json!({
                    "legend": { "displayMode": "list", "placement": "bottom", "showLegend": true },
                    "tooltip": { "mode": "multi", "sort": "desc" },
                }),
                json!([]),
            )
        }
        PanelType::Stat => {
            defaults["color"] = json!({ "mode": "thresholds" });
            (
                "stat",
                json!({
                    "reduceOptions": { "calcs": ["lastNotNull"], "fields": "", "values": false },
                    "colorMode": "value",
                    "graphMode": "area",
                    "textMode": "auto",
                }),
                json!([]),
            )
        }
        PanelType::Row => return Ok(export_row(panel, id)),
        PanelType::Table => {
            // One row per label set: merge the queries, drop the timestamp, name each value
            // column after its query's legend.
            let renames: serde_json::Map<String, Value> = panel
                .targets
                .iter()
                .map(|target| {
                    (
                        format!("Value #{}", target.ref_id),
                        json!(
                            target
                                .legend
                                .clone()
                                .unwrap_or_else(|| target.ref_id.clone())
                        ),
                    )
                })
                .collect();
            (
                "table",
                json!({ "showHeader": true, "cellHeight": "sm" }),
                json!([
                    { "id": "merge", "options": {} },
                    {
                        "id": "organize",
                        "options": { "excludeByName": { "Time": true }, "renameByName": renames },
                    },
                ]),
            )
        }
    };

    Ok(json!({
        "id": id,
        "type": kind,
        "title": panel.title,
        "description": panel.description,
        "gridPos": panel.grid_pos,
        "datasource": datasource,
        "targets": targets,
        "fieldConfig": { "defaults": defaults, "overrides": overrides },
        "options": options,
        "transformations": transformations,
    }))
}

/// A Grafana row: an expanded section header (its panels follow it at top level).
fn export_row(panel: &Panel, id: u64) -> Value {
    json!({
        "id": id,
        "type": "row",
        "title": panel.title,
        "collapsed": false,
        "gridPos": { "x": 0, "y": panel.grid_pos.y, "w": 24, "h": 1 },
        "panels": [],
    })
}

fn grafana_color(color: SeriesColor) -> &'static str {
    match color {
        SeriesColor::Neutral => "text",
        SeriesColor::Accent => "yellow",
        SeriesColor::Success => "green",
        SeriesColor::Warning => "orange",
        SeriesColor::Error => "red",
        SeriesColor::Info => "blue",
    }
}

/// A Grafana the user named, reached through the server's egress policy.
pub struct GrafanaClient {
    http: reqwest::Client,
    base: Url,
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrafanaDatasource {
    pub uid: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GrafanaFolder {
    pub uid: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct PushedDashboard {
    pub title: String,
    pub url: String,
}

#[derive(Deserialize)]
struct RawDatasource {
    uid: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "isDefault", default)]
    is_default: bool,
}

impl GrafanaClient {
    pub fn new(url: &str, token: &str, egress: &Arc<EgressPolicy>) -> Result<Self, MetricsError> {
        let mut base = Url::parse(url.trim())
            .map_err(|_| MetricsError::bad_request("Grafana URL is not a URL"))?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(MetricsError::bad_request(
                "Grafana URL must be http or https",
            ));
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(MetricsError::bad_request(
                "put the service-account token in the token field, not in the URL",
            ));
        }
        egress.check_url(&base).map_err(MetricsError::BadRequest)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        if token.trim().is_empty() {
            return Err(MetricsError::bad_request(
                "a Grafana service-account token is required",
            ));
        }
        let http = egress
            .client_builder()
            .timeout(GRAFANA_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| MetricsError::Unavailable(e.to_string()))?;
        Ok(Self {
            http,
            base,
            token: token.trim().to_string(),
        })
    }

    pub async fn datasources(&self) -> Result<Vec<GrafanaDatasource>, MetricsError> {
        let raw: Vec<RawDatasource> = self.get("api/datasources").await?;
        Ok(raw
            .into_iter()
            .filter(|ds| ds.kind == "prometheus")
            .map(|ds| GrafanaDatasource {
                uid: ds.uid,
                name: ds.name,
                is_default: ds.is_default,
            })
            .collect())
    }

    pub async fn folders(&self) -> Result<Vec<GrafanaFolder>, MetricsError> {
        self.get("api/folders").await
    }

    /// Creates or overwrites the dashboard (same uid) and returns its Grafana URL.
    pub async fn push(
        &self,
        dashboard: Value,
        folder_uid: Option<&str>,
    ) -> Result<PushedDashboard, MetricsError> {
        let title = dashboard["title"].as_str().unwrap_or_default().to_string();
        let mut body = json!({
            "dashboard": dashboard,
            "overwrite": true,
            "message": "Pushed from the OctoFHIR console",
        });
        if let Some(folder) = folder_uid.filter(|f| !f.is_empty()) {
            body["folderUid"] = json!(folder);
        }
        let response = self
            .http
            .post(self.url("api/dashboards/db"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(transport)?;
        let answer: Value = read(response).await?;
        let path = answer["url"].as_str().unwrap_or_default();
        let url = self.url(path.trim_start_matches('/')).to_string();
        Ok(PushedDashboard { title, url })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, MetricsError> {
        let response = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        read(response).await
    }

    fn url(&self, path: &str) -> Url {
        self.base.join(path).unwrap_or_else(|_| self.base.clone())
    }
}

async fn read<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, MetricsError> {
    let status = response.status();
    if status.is_redirection() {
        return Err(MetricsError::Unavailable(
            "Grafana redirected the request; use its base URL, including any sub-path".into(),
        ));
    }
    if status.as_u16() == 401 {
        return Err(MetricsError::Unavailable(
            "Grafana rejected the token; use a service-account token".into(),
        ));
    }
    if status.as_u16() == 403 {
        return Err(MetricsError::Unavailable(
            "the token's service account lacks permission (it needs the Editor role on the folder)"
                .into(),
        ));
    }
    let bytes = response.bytes().await.map_err(transport)?;
    if !status.is_success() {
        let message = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("Grafana answered {status}"));
        return Err(MetricsError::Unavailable(message));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| MetricsError::Unavailable("Grafana's answer is not the expected JSON".into()))
}

fn transport(error: reqwest::Error) -> MetricsError {
    if error.is_timeout() {
        MetricsError::Unavailable("Grafana did not answer in time".into())
    } else {
        MetricsError::Unavailable(format!("cannot reach Grafana ({})", error.without_url()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prometheus::dashboards::builtin;
    use crate::prometheus::promql::parse_selector;

    const SELECTOR: &str = r#"{job="octofhir"}"#;

    fn first_panel(out: &Value) -> &Value {
        out["panels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["type"] != "row")
            .unwrap()
    }

    fn system() -> Value {
        let scope = parse_selector(SELECTOR).unwrap();
        export(&builtin()[0], &scope, SELECTOR, None).unwrap()
    }

    #[test]
    fn export_declares_a_datasource_input() {
        let out = system();
        assert_eq!(out["__inputs"][0]["name"], DATASOURCE_INPUT);
        assert_eq!(out["uid"], "octofhir-system");
        assert_eq!(out["graphTooltip"], 1);
        assert_eq!(first_panel(&out)["datasource"]["uid"], DATASOURCE_REF);
        assert_eq!(out["panels"][0]["type"], "row");
    }

    #[test]
    fn export_scopes_queries_and_keeps_grafana_variables() {
        let out = system();
        for panel in out["panels"].as_array().unwrap() {
            if panel["type"] == "row" {
                continue;
            }
            for target in panel["targets"].as_array().unwrap() {
                let expr = target["expr"].as_str().unwrap();
                assert!(expr.contains(r#"job="octofhir""#), "{expr}");
                assert!(expr.contains("$instance"), "{expr}");
                assert!(!expr.contains("__octofhir_var"), "{expr}");
            }
        }
        let variable = &out["templating"]["list"][0];
        assert_eq!(
            variable["definition"],
            r#"label_values(up{job="octofhir"}, instance)"#
        );
        assert_eq!(variable["allValue"], ".*");
    }

    /// `deploy/observability/grafana/` holds the built-in dashboards for Grafana users who import
    /// them by hand. Regenerate with `UPDATE_GRAFANA_DASHBOARDS=1 cargo test -p octofhir-server
    /// --lib grafana_files_are_current`.
    #[test]
    fn grafana_files_are_current() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/observability/grafana");
        let scope = parse_selector(SELECTOR).unwrap();
        for dashboard in builtin() {
            let json = export(&dashboard, &scope, SELECTOR, None).unwrap();
            let text = format!("{}\n", serde_json::to_string_pretty(&json).unwrap());
            let path = dir.join(format!("{}.json", dashboard.id.as_deref().unwrap()));
            if std::env::var_os("UPDATE_GRAFANA_DASHBOARDS").is_some() {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&path, &text).unwrap();
            }
            let stored = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                stored == text,
                "{} is stale; regenerate with UPDATE_GRAFANA_DASHBOARDS=1",
                path.display()
            );
        }
    }

    #[test]
    fn push_export_pins_the_datasource() {
        let scope = parse_selector(SELECTOR).unwrap();
        let out = export(&builtin()[0], &scope, SELECTOR, Some("prom-1")).unwrap();
        assert!(out.get("__inputs").is_none());
        assert_eq!(first_panel(&out)["datasource"]["uid"], "prom-1");
    }

    #[test]
    fn tables_are_instant_and_merged() {
        let out = system();
        let table = out["panels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["title"] == "Routes" && p["type"] == "table")
            .unwrap();
        assert_eq!(table["targets"][0]["instant"], true);
        assert_eq!(table["transformations"][0]["id"], "merge");
        assert_eq!(
            table["transformations"][1]["options"]["renameByName"]["Value #A"],
            "Req/s"
        );
        let units = table["fieldConfig"]["overrides"].as_array().unwrap();
        assert!(
            units.iter().any(|o| o["matcher"]["options"] == "5xx"
                && o["properties"][0]["value"] == "percentunit")
        );
    }

    #[test]
    fn grafana_url_must_pass_egress_and_carry_no_credentials() {
        let strict = Arc::new(EgressPolicy::new(false, &[]).unwrap());
        assert!(GrafanaClient::new("http://127.0.0.1:3000", "t", &strict).is_err());
        let open = Arc::new(EgressPolicy::permissive());
        assert!(GrafanaClient::new("http://admin:pw@grafana:3000", "t", &open).is_err());
        assert!(GrafanaClient::new("http://grafana:3000", " ", &open).is_err());
        assert!(GrafanaClient::new("ftp://grafana", "t", &open).is_err());
        assert!(GrafanaClient::new("http://grafana:3000/sub", "t", &open).is_ok());
    }
}
