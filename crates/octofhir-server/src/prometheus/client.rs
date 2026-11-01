//! Client for the Prometheus HTTP API the console's dashboards read from.
//!
//! Every query is scoped to the configured selector before it leaves (see [`super::promql`]), and
//! ranges and steps are clamped so one dashboard can't ask Prometheus for millions of points.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

use promql_parser::label::Matcher;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{RequestBuilder, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::MetricsError;
use super::promql;
use crate::config::PrometheusConfig;

/// A rate window shorter than four scrapes often holds fewer than two samples and comes back
/// empty, so suggested windows never go below this many scrape intervals.
const RATE_WINDOW_SCRAPES: u64 = 4;

/// Prometheus-compatible server scoped to one deployment's series.
#[derive(Clone)]
pub struct PrometheusClient {
    http: reqwest::Client,
    base: Url,
    scope: Vec<Matcher>,
    selector: String,
    scrape_interval: Duration,
    max_range: Duration,
    max_points: u64,
    timeout: Duration,
    /// Basic-auth user and password, sent per request (reqwest has no default for it).
    basic: Option<(String, Option<String>)>,
}

/// A step and range after clamping, returned so the console draws the axis it actually got.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeWindow {
    pub start: f64,
    pub end: f64,
    pub step: f64,
}

/// One scraped target of the configured job.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub instance: String,
    pub up: bool,
    pub version: Option<String>,
}

/// A metric name and what Prometheus knows about it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub help: Option<String>,
    pub unit: Option<String>,
}

/// A syntax error with its 1-based position, when the server could tell it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Deserialize)]
struct Envelope {
    status: String,
    #[serde(default)]
    data: Value,
    #[serde(rename = "errorType", default)]
    error_type: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct MetadataEntry {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    unit: Option<String>,
}

impl PrometheusClient {
    pub fn new(config: &PrometheusConfig) -> Result<Self, String> {
        let mut base = Url::parse(&config.url).map_err(|e| e.to_string())?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let scope = promql::parse_selector(&config.selector)?;

        let mut headers = HeaderMap::new();
        for (name, value) in &config.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| format!("header {name:?}: {e}"))?;
            let mut value =
                HeaderValue::from_str(value).map_err(|e| format!("header {name}: {e}"))?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        if let Some(token) = &config.bearer_token {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| "bearer_token is not a valid header value".to_string())?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(config.timeout() + Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            http,
            base,
            scope,
            selector: config.selector.trim().to_string(),
            scrape_interval: config.scrape_interval(),
            max_range: config.max_range(),
            max_points: config.max_points,
            basic: config
                .basic_username
                .clone()
                .map(|user| (user, config.basic_password.clone())),
            timeout: config.timeout(),
        })
    }

    pub fn scrape_interval(&self) -> Duration {
        self.scrape_interval
    }

    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Clamps a requested range to `max_range` (keeping its end) and picks a step no finer than
    /// the scrape interval that keeps each series under `max_points`.
    pub fn window(
        &self,
        start: f64,
        end: f64,
        step: Option<f64>,
    ) -> Result<RangeWindow, MetricsError> {
        if !(start.is_finite() && end.is_finite()) || end <= start {
            return Err(MetricsError::bad_request("end must be after start"));
        }
        let start = start.max(end - self.max_range.as_secs_f64());
        let span = end - start;
        let floor = self.scrape_interval.as_secs_f64();
        let fit = (span / self.max_points as f64).ceil();
        let step = step
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(0.0)
            .max(floor)
            .max(fit);
        Ok(RangeWindow { start, end, step })
    }

    /// Rate window Grafana would call `$__rate_interval`: at least four scrapes and one step.
    pub fn rate_window(&self, step: f64) -> u64 {
        let scrapes = self.scrape_interval.as_secs() * RATE_WINDOW_SCRAPES;
        scrapes.max(step.ceil() as u64)
    }

    /// `/api/v1/query_range` for a scoped query; returns Prometheus' `data` object.
    pub async fn query_range(
        &self,
        query: &str,
        window: RangeWindow,
    ) -> Result<Value, MetricsError> {
        let scoped = self.scoped(query)?;
        let form = [
            ("query", scoped),
            ("start", window.start.to_string()),
            ("end", window.end.to_string()),
            ("step", window.step.to_string()),
            ("timeout", format!("{}s", self.timeout.as_secs())),
        ];
        self.call(self.post("api/v1/query_range").form(&form)).await
    }

    /// `/api/v1/query` for a scoped query at `time` (now when absent).
    pub async fn query(&self, query: &str, time: Option<f64>) -> Result<Value, MetricsError> {
        let scoped = self.scoped(query)?;
        let mut form = vec![
            ("query", scoped),
            ("timeout", format!("{}s", self.timeout.as_secs())),
        ];
        if let Some(time) = time.filter(|t| t.is_finite()) {
            form.push(("time", time.to_string()));
        }
        self.call(self.post("api/v1/query").form(&form)).await
    }

    /// Targets of the configured job, with the version each instance reports.
    pub async fn targets(&self) -> Result<Vec<Target>, MetricsError> {
        let up = self.query("up", None).await?;
        let versions = self
            .query("octofhir_build_info", None)
            .await
            .unwrap_or(Value::Null);
        let mut by_instance: BTreeMap<String, Option<String>> = BTreeMap::new();
        for sample in samples(&versions) {
            if let Some(instance) = sample.pointer("/metric/instance").and_then(Value::as_str) {
                let version = sample.pointer("/metric/version").and_then(Value::as_str);
                by_instance.insert(instance.to_string(), version.map(str::to_string));
            }
        }
        let mut targets: Vec<Target> = samples(&up)
            .filter_map(|sample| {
                let instance = sample.pointer("/metric/instance")?.as_str()?.to_string();
                let value = sample.pointer("/value/1")?.as_str()?;
                Some(Target {
                    version: by_instance.get(&instance).cloned().flatten(),
                    up: value == "1",
                    instance,
                })
            })
            .collect();
        targets.sort_by(|a, b| a.instance.cmp(&b.instance));
        Ok(targets)
    }

    /// Metric names this deployment exports, with type and help where Prometheus has them.
    pub async fn metrics(&self) -> Result<Vec<MetricInfo>, MetricsError> {
        let names = self.label_values("__name__", None).await?;
        let metadata: BTreeMap<String, Vec<MetadataEntry>> = self
            .call(self.get("api/v1/metadata"))
            .await
            .ok()
            .and_then(|data| serde_json::from_value(data).ok())
            .unwrap_or_default();
        Ok(names
            .into_iter()
            .map(|name| {
                let meta = metadata
                    .get(&name)
                    .or_else(|| metadata.get(base_metric(&name)));
                let first = meta.and_then(|entries| entries.first());
                MetricInfo {
                    kind: first.and_then(|m| m.kind.clone()),
                    help: first.and_then(|m| m.help.clone()).filter(|h| !h.is_empty()),
                    unit: first.and_then(|m| m.unit.clone()).filter(|u| !u.is_empty()),
                    name,
                }
            })
            .collect())
    }

    /// Label names of this deployment's series, or of one metric's.
    pub async fn labels(&self, metric: Option<&str>) -> Result<Vec<String>, MetricsError> {
        let request = self
            .get("api/v1/labels")
            .query(&[("match[]", self.series_match(metric)?)]);
        let data = self.call(request).await?;
        Ok(strings(data))
    }

    /// Values of one label across this deployment's series, or one metric's.
    pub async fn label_values(
        &self,
        label: &str,
        metric: Option<&str>,
    ) -> Result<Vec<String>, MetricsError> {
        if !is_label_name(label) {
            return Err(MetricsError::bad_request(format!(
                "{label:?} is not a label name"
            )));
        }
        let request = self
            .get(&format!("api/v1/label/{label}/values"))
            .query(&[("match[]", self.series_match(metric)?)]);
        let data = self.call(request).await?;
        Ok(strings(data))
    }

    /// Checks a query's syntax. `Ok(None)` means it parses and can be scoped.
    pub async fn check(&self, query: &str) -> Result<Option<ParseError>, MetricsError> {
        let local = match promql::scope_query(query, &self.scope) {
            Ok(_) => return Ok(None),
            Err(message) => message,
        };
        // promql-parser reports errors without positions; Prometheus' own parser has them.
        let remote = self
            .call(self.post("api/v1/parse_query").form(&[("query", query)]))
            .await;
        Ok(Some(match remote {
            Err(MetricsError::Query(message)) => positioned(&message),
            Ok(_) => ParseError {
                message: format!("the server cannot scope this query: {local}"),
                line: None,
                column: None,
            },
            Err(_) => ParseError {
                message: local,
                line: None,
                column: None,
            },
        }))
    }

    /// The selector's matchers, added to every query.
    pub fn matchers(&self) -> &[Matcher] {
        &self.scope
    }

    fn scoped(&self, query: &str) -> Result<String, MetricsError> {
        promql::scope_query(query, &self.scope).map_err(MetricsError::Query)
    }

    /// `match[]` value: the selector, narrowed to one metric when given.
    fn series_match(&self, metric: Option<&str>) -> Result<String, MetricsError> {
        match metric {
            None => Ok(self.selector.clone()),
            Some(name) if is_metric_name(name) => Ok(format!("{name}{}", self.selector)),
            Some(name) => Err(MetricsError::bad_request(format!(
                "{name:?} is not a metric name"
            ))),
        }
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.authorize(self.http.get(self.url(path)))
    }

    fn post(&self, path: &str) -> RequestBuilder {
        self.authorize(self.http.post(self.url(path)))
    }

    fn url(&self, path: &str) -> Url {
        self.base.join(path).unwrap_or_else(|_| self.base.clone())
    }

    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.basic {
            Some((user, password)) => request.basic_auth(user, password.as_deref()),
            None => request,
        }
    }

    async fn call(&self, request: RequestBuilder) -> Result<Value, MetricsError> {
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                MetricsError::Timeout
            } else {
                MetricsError::Unavailable(without_url(e))
            }
        })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MetricsError::Unavailable(without_url(e)))?;
        let envelope: Envelope = match serde_json::from_slice(&bytes) {
            Ok(envelope) => envelope,
            Err(_) if status.as_u16() == 401 || status.as_u16() == 403 => {
                return Err(MetricsError::Unavailable(
                    "Prometheus rejected the configured credentials".into(),
                ));
            }
            Err(_) => {
                return Err(MetricsError::Unavailable(format!(
                    "Prometheus answered {status} without an API response"
                )));
            }
        };
        if envelope.status == "success" {
            return Ok(envelope.data);
        }
        let message = envelope
            .error
            .unwrap_or_else(|| format!("Prometheus answered {status}"));
        Err(match envelope.error_type.as_deref() {
            Some("bad_data") => MetricsError::Query(message),
            Some("execution") => MetricsError::Execution(message),
            Some("timeout") | Some("canceled") => MetricsError::Timeout,
            _ => MetricsError::Unavailable(message),
        })
    }
}

fn samples(data: &Value) -> impl Iterator<Item = &Value> {
    data.get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn strings(data: Value) -> Vec<String> {
    let mut seen = HashSet::new();
    match data {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .filter(|item| seen.insert(item.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Histogram and summary series carry metadata under their base name.
fn base_metric(name: &str) -> &str {
    ["_bucket", "_sum", "_count"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
}

/// `line:column:` and the message after it, wherever Prometheus puts them (it prefixes API
/// errors with `invalid parameter "query": `).
static ERROR_POSITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+):(\d+):\s*(?:parse error:\s*)?(.*)").expect("valid regex"));

/// Reads the position Prometheus reports for a parse error.
fn positioned(message: &str) -> ParseError {
    match ERROR_POSITION.captures(message) {
        Some(captures) => ParseError {
            message: captures[3].trim().to_string(),
            line: captures[1].parse().ok(),
            column: captures[2].parse().ok(),
        },
        None => ParseError {
            message: message.to_string(),
            line: None,
            column: None,
        },
    }
}

/// Transport errors print the request URL; keep it (and any credentials in it) out of
/// responses.
fn without_url(error: reqwest::Error) -> String {
    let connect = error.is_connect();
    let message = error.without_url().to_string();
    if connect {
        format!("cannot reach Prometheus ({message})")
    } else {
        message
    }
}

pub fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

pub fn is_label_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> PrometheusClient {
        PrometheusClient::new(&PrometheusConfig {
            url: "http://prometheus:9090".into(),
            selector: r#"{job="octofhir"}"#.into(),
            scrape_interval_secs: 15,
            timeout_secs: 10,
            max_range_days: 1,
            max_points: 100,
            bearer_token: None,
            basic_username: None,
            basic_password: None,
            headers: BTreeMap::new(),
        })
        .unwrap()
    }

    #[test]
    fn window_keeps_steps_above_scrape_interval() {
        let window = client().window(0.0, 600.0, Some(1.0)).unwrap();
        assert_eq!(window.step, 15.0);
    }

    #[test]
    fn window_coarsens_steps_to_fit_max_points() {
        let window = client().window(0.0, 3600.0 * 10.0, None).unwrap();
        assert_eq!(window.step, 360.0);
    }

    #[test]
    fn window_clamps_range_to_max_range_keeping_end() {
        let end = 10.0 * 86_400.0;
        let window = client().window(0.0, end, None).unwrap();
        assert_eq!(window.start, end - 86_400.0);
        assert!(client().window(5.0, 5.0, None).is_err());
    }

    #[test]
    fn rate_window_covers_four_scrapes() {
        assert_eq!(client().rate_window(15.0), 60);
        assert_eq!(client().rate_window(300.0), 300);
    }

    #[test]
    fn reads_prometheus_error_positions() {
        let error = positioned("1:9: parse error: unexpected end of input");
        assert_eq!((error.line, error.column), (Some(1), Some(9)));
        assert_eq!(error.message, "unexpected end of input");
        assert_eq!(positioned("boom").line, None);
        let prefixed =
            positioned(r#"invalid parameter "query": 1:15: parse error: unexpected end of input"#);
        assert_eq!((prefixed.line, prefixed.column), (Some(1), Some(15)));
        assert_eq!(prefixed.message, "unexpected end of input");
    }

    #[test]
    fn validates_names() {
        assert!(is_metric_name("octofhir_http_requests_total"));
        assert!(is_metric_name("job:rate5m"));
        assert!(!is_metric_name("1abc"));
        assert!(!is_metric_name("a{b}"));
        assert!(is_label_name("status_class"));
        assert!(!is_label_name("a:b"));
        assert_eq!(base_metric("x_bucket"), "x");
    }
}
