//! Dashboard documents: the shape shared by the built-in dashboards, `Dashboard` resources the
//! console saves, and the Grafana export.
//!
//! The model follows Grafana's on purpose (24-column `gridPos`, Grafana unit ids, `$variables`),
//! so a dashboard converts to Grafana JSON without losing anything.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// Built-in dashboards, shipped with the server and read-only in the console.
const BUILTIN: &[&str] = &[include_str!("../../dashboards/system.json")];

/// Most panels one dashboard may hold.
pub const MAX_PANELS: usize = 100;
/// Most queries one panel may run.
pub const MAX_TARGETS: usize = 10;
pub const GRID_COLUMNS: u32 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    /// Built-in id, or the `Dashboard` resource id.
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub time: TimeRange,
    #[serde(default)]
    pub variables: Vec<Variable>,
    /// How hovering one chart shows on the others.
    #[serde(default)]
    pub cursor_sync: CursorSync,
    #[serde(default)]
    pub panels: Vec<Panel>,
}

/// Grafana's "graph tooltip" setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorSync {
    /// Each chart on its own.
    Off,
    /// Every chart follows the cursor; only the hovered one shows values.
    #[default]
    Crosshair,
    /// Every chart follows the cursor and shows its values.
    Tooltip,
}

/// Relative time range in Grafana's notation, e.g. `now-1h` to `now`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: String,
    pub to: String,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self {
            from: "now-1h".into(),
            to: "now".into(),
        }
    }
}

/// A label-value variable: `$name` in queries, choices from `label_values(metric, label)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The label whose values are offered.
    pub label_name: String,
    /// Metric to read the values from; any of this deployment's series when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default)]
    pub multi: bool,
    /// Offer "All", which stands for `.*` (use the variable with `=~`).
    #[serde(default)]
    pub include_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelType {
    Timeseries,
    Stat,
    Table,
    /// Section header spanning the grid; collapses the panels below it up to the next row.
    Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DrawStyle {
    #[default]
    Line,
    Bars,
}

/// Semantic series colours; the console maps them to theme tokens, Grafana to its named colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesColor {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
    Info,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GridPos {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub ref_id: String,
    pub expr: String,
    /// Series name template, e.g. `{{status_class}}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<String>,
    /// Overrides the panel's unit for this query (table columns of different kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub value: f64,
    pub color: SeriesColor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: PanelType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub grid_pos: GridPos,
    #[serde(default)]
    pub targets: Vec<Target>,
    /// Grafana unit id: `s`, `ms`, `reqps`, `percentunit`, `bytes`, `short`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Timeseries only.
    #[serde(default)]
    pub draw_style: DrawStyle,
    /// Timeseries only: stack series.
    #[serde(default)]
    pub stack: bool,
    /// Fixed colours by series name (the rendered legend).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, SeriesColor>,
    /// Stat only: value colour from the highest threshold reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thresholds: Vec<Threshold>,
}

impl Dashboard {
    /// Checks what the console and the Grafana export rely on.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("title is required".into());
        }
        if self.panels.len() > MAX_PANELS {
            return Err(format!("at most {MAX_PANELS} panels"));
        }
        let mut variable_names = HashSet::new();
        for variable in &self.variables {
            if !super::client::is_label_name(&variable.name) || variable.name.starts_with("__") {
                return Err(format!("variable name {:?} is not valid", variable.name));
            }
            if !variable_names.insert(variable.name.as_str()) {
                return Err(format!("variable {:?} is defined twice", variable.name));
            }
            if !super::client::is_label_name(&variable.label_name) {
                return Err(format!("{:?} is not a label name", variable.label_name));
            }
            if let Some(metric) = &variable.metric
                && !super::client::is_metric_name(metric)
            {
                return Err(format!("{metric:?} is not a metric name"));
            }
        }
        let mut panel_ids = HashSet::new();
        for panel in &self.panels {
            if !panel_ids.insert(panel.id.as_str()) {
                return Err(format!("panel id {:?} is used twice", panel.id));
            }
            let pos = panel.grid_pos;
            if pos.w == 0 || pos.h == 0 || pos.x + pos.w > GRID_COLUMNS {
                return Err(format!(
                    "panel {:?} does not fit the 24-column grid",
                    panel.title
                ));
            }
            if panel.kind == PanelType::Row {
                if !panel.targets.is_empty() {
                    return Err(format!("row {:?} cannot have queries", panel.title));
                }
                continue;
            }
            if panel.targets.is_empty() || panel.targets.len() > MAX_TARGETS {
                return Err(format!(
                    "panel {:?} needs 1 to {MAX_TARGETS} queries",
                    panel.title
                ));
            }
        }
        Ok(())
    }
}

/// The built-in dashboards, parsed. A malformed one is a build defect, caught by the tests.
pub fn builtin() -> Vec<Dashboard> {
    BUILTIN
        .iter()
        .filter_map(|source| match serde_json::from_str::<Dashboard>(source) {
            Ok(dashboard) => Some(dashboard),
            Err(e) => {
                tracing::error!(error = %e, "Built-in dashboard does not parse");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prometheus::promql::{Variables, parse_selector, scope_query};

    #[test]
    fn builtin_dashboards_are_valid_and_their_queries_scope() {
        let scope = parse_selector(r#"{job="octofhir"}"#).unwrap();
        let dashboards = builtin();
        assert_eq!(dashboards.len(), BUILTIN.len());
        for dashboard in dashboards {
            dashboard.validate().unwrap();
            assert!(dashboard.id.is_some(), "{}", dashboard.title);
            let mut user = BTreeMap::new();
            for variable in &dashboard.variables {
                user.insert(variable.name.clone(), ".*".to_string());
            }
            let vars = Variables::new(60, 15, 3600).with_user(&user);
            for panel in &dashboard.panels {
                for target in &panel.targets {
                    let expanded = vars.expand(&target.expr).query;
                    scope_query(&expanded, &scope)
                        .unwrap_or_else(|e| panic!("{} / {}: {e}", panel.title, target.expr));
                }
            }
        }
    }

    #[test]
    fn rejects_panels_off_the_grid() {
        let mut dashboard = builtin().remove(0);
        dashboard.panels[0].grid_pos.x = 20;
        dashboard.panels[0].grid_pos.w = 8;
        assert!(dashboard.validate().is_err());
    }
}
