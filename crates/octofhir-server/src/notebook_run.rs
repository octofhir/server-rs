//! Headless notebook execution — runs the whole cell DAG server-side by calling
//! the server's own engine endpoints in-process (same engines the browser uses),
//! in dependency order with `${scope}` templating. For scheduled reports / CI /
//! export. See docs/ui-notebooks-plan.md §7d (v2). Markdown is interpolated;
//! pipeline/chart/input are not executed headless (kept as-is).

use crate::server::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use octofhir_api::ApiError;
use octofhir_notebook::{Cell, Notebook, Output};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

// ─────────────────────────── templating ───────────────────────────

fn resolve_path(scope: &Map<String, Value>, path: &str) -> Option<Value> {
    let mut cur = Value::Object(scope.clone());
    for seg in path.split('.') {
        for part in seg.split('[') {
            let key = part.trim_end_matches(']');
            if key.is_empty() {
                continue;
            }
            cur = if let Ok(idx) = key.parse::<usize>() {
                cur.get(idx)?.clone()
            } else {
                cur.get(key)?.clone()
            };
        }
    }
    Some(cur)
}

/// Substitute `${name}` / `${name.path}` tokens from the scope.
fn interpolate(src: &str, scope: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'{'
            && let Some(end) = src[i + 2..].find('}')
        {
            let path = &src[i + 2..i + 2 + end];
            match resolve_path(scope, path) {
                Some(Value::String(s)) => out.push_str(&s),
                Some(v @ (Value::Object(_) | Value::Array(_))) => out.push_str(&v.to_string()),
                Some(v) => out.push_str(&v.to_string()),
                None => out.push_str(&src[i..i + 2 + end + 1]),
            }
            i += 2 + end + 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn deep_interp(v: &Value, scope: &Map<String, Value>) -> Value {
    match v {
        Value::String(s) => Value::String(interpolate(s, scope)),
        Value::Array(a) => Value::Array(a.iter().map(|x| deep_interp(x, scope)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), deep_interp(x, scope)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ─────────────────────────── DAG order ───────────────────────────

fn ref_names(cell: &Cell) -> Vec<String> {
    let mut names = Vec::new();
    let mut scan = |v: &Value| {
        fn walk(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::String(s) => {
                    let b = s.as_bytes();
                    let mut i = 0;
                    while i < b.len() {
                        if b[i] == b'$'
                            && i + 1 < b.len()
                            && b[i + 1] == b'{'
                            && let Some(end) = s[i + 2..].find('}')
                        {
                            let path = &s[i + 2..i + 2 + end];
                            let root: String =
                                path.split(['.', '[']).next().unwrap_or("").to_string();
                            if !root.is_empty() {
                                out.push(root);
                            }
                            i += 2 + end + 1;
                            continue;
                        }
                        i += 1;
                    }
                }
                Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                Value::Object(o) => o.values().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        walk(v, &mut names);
    };
    scan(&cell.source);
    if !cell.config.is_null() {
        scan(&cell.config);
    }
    // chart / pipeline explicit inputs are name refs
    if cell.cell_type == "chart"
        && let Some(n) = cell.source.get("inputCell").and_then(|v| v.as_str())
    {
        names.push(n.to_string());
    }
    if cell.cell_type == "pipeline"
        && let Some(n) = cell.source.get("input").and_then(|v| v.as_str())
    {
        names.push(n.to_string());
    }
    names
}

/// Cell indices in dependency order (producers before consumers).
fn topo_order(nb: &Notebook) -> Vec<usize> {
    let name_to_idx: HashMap<&str, usize> = nb
        .cells
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.name.as_deref().map(|n| (n, i)))
        .collect();
    let deps: Vec<Vec<usize>> = nb
        .cells
        .iter()
        .map(|c| {
            ref_names(c)
                .iter()
                .filter_map(|n| name_to_idx.get(n.as_str()).copied())
                .collect()
        })
        .collect();

    let mut visited = vec![false; nb.cells.len()];
    let mut order = Vec::new();
    fn visit(i: usize, deps: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
        if visited[i] {
            return;
        }
        visited[i] = true;
        for &d in &deps[i] {
            if d != i {
                visit(d, deps, visited, order);
            }
        }
        order.push(i);
    }
    for i in 0..nb.cells.len() {
        visit(i, &deps, &mut visited, &mut order);
    }
    order
}

// ─────────────────────────── output parsing ───────────────────────────

fn out(kind: &str, rest: Value) -> Output {
    Output {
        kind: kind.to_string(),
        rest: rest.as_object().cloned().unwrap_or_default(),
    }
}

fn err_out(msg: impl Into<String>) -> Output {
    out(
        "error",
        json!({ "severity": "error", "message": msg.into() }),
    )
}

fn parse_fhirpath(v: &Value) -> Output {
    let mut data = Vec::new();
    if let Some(params) = v.get("parameter").and_then(|p| p.as_array()) {
        for entry in params {
            if entry.get("name").and_then(|n| n.as_str()) == Some("metadata") {
                continue;
            }
            if let Some(obj) = entry.as_object() {
                if let Some((_, val)) = obj.iter().find(|(k, _)| k.starts_with("value")) {
                    data.push(val.clone());
                } else if let Some(res) = obj.get("resource") {
                    data.push(res.clone());
                }
            }
        }
    }
    out("value", json!({ "data": data }))
}

fn parse_sql(v: &Value) -> Output {
    let columns = v.get("columns").cloned().unwrap_or_else(|| json!([]));
    let rows = v.get("rows").cloned().unwrap_or_else(|| json!([]));
    let row_count = v
        .get("rowCount")
        .and_then(|n| n.as_u64())
        .unwrap_or_else(|| rows.as_array().map(|a| a.len() as u64).unwrap_or(0));
    out(
        "table",
        json!({ "columns": columns, "rows": rows,
                "meta": { "rowCount": row_count, "truncated": false } }),
    )
}

fn parse_sof(v: &Value) -> Output {
    let params = v.get("parameter").and_then(|p| p.as_array());
    let mut columns: Vec<String> = Vec::new();
    if let Some(params) = params {
        for e in params {
            if e.get("name").and_then(|n| n.as_str()) == Some("columns")
                && let Some(parts) = e.get("part").and_then(|p| p.as_array())
            {
                for part in parts {
                    if let Some(n) = part.get("name").and_then(|n| n.as_str()) {
                        columns.push(n.to_string());
                    }
                }
            }
        }
    }
    let mut row_objs: Vec<Value> = Vec::new();
    if let Some(params) = params
        && let Some(rows_entry) = params
            .iter()
            .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("rows"))
        && let Some(data) = rows_entry
            .pointer("/resource/data")
            .and_then(|d| d.as_str())
        && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
        && let Ok(parsed) = serde_json::from_slice::<Vec<Value>>(&bytes)
    {
        row_objs = parsed;
    }
    if columns.is_empty()
        && let Some(first) = row_objs.first().and_then(|r| r.as_object())
    {
        columns = first.keys().cloned().collect();
    }
    let rows: Vec<Value> = row_objs
        .iter()
        .map(|r| {
            Value::Array(
                columns
                    .iter()
                    .map(|c| r.get(c).cloned().unwrap_or(Value::Null))
                    .collect(),
            )
        })
        .collect();
    let n = rows.len() as u64;
    out(
        "table",
        json!({ "columns": columns, "rows": rows,
                "meta": { "rowCount": n, "truncated": false } }),
    )
}

fn parse_cql(v: &Value) -> Output {
    let params = v.get("parameter").and_then(|p| p.as_array());
    let try_json = |s: &str| serde_json::from_str::<Value>(s).unwrap_or_else(|_| json!(s));
    if let Some(params) = params {
        if let Some(result) = params
            .iter()
            .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("result"))
            && let Some(parts) = result.get("part").and_then(|p| p.as_array())
        {
            let mut defines = Map::new();
            for part in parts {
                if let (Some(name), Some(vs)) = (
                    part.get("name").and_then(|n| n.as_str()),
                    part.get("valueString").and_then(|s| s.as_str()),
                ) {
                    defines.insert(name.to_string(), try_json(vs));
                }
            }
            return out("json", json!({ "data": Value::Object(defines) }));
        }
        if let Some(ret) = params
            .iter()
            .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("return"))
            .and_then(|e| e.get("valueString"))
            .and_then(|s| s.as_str())
        {
            return out("value", json!({ "data": [try_json(ret)] }));
        }
    }
    out("value", json!({ "data": [] }))
}

// ─────────────────────────── per-cell request ───────────────────────────

const CQL_LIB: [&str; 3] = ["library", "using", "define"];

async fn run_cell(
    client: &reqwest::Client,
    base: &str,
    cell: &Cell,
    scope: &Map<String, Value>,
) -> Output {
    let src_str = cell.source.as_str().unwrap_or("").to_string();
    match cell.cell_type.as_str() {
        "markdown" => out("markdown", json!({ "text": interpolate(&src_str, scope) })),
        "fhirpath" => {
            let body = json!({ "resourceType": "Parameters",
                "parameter": [{ "name": "expression", "valueString": interpolate(&src_str, scope) }] });
            post_parse(
                client,
                &format!("{base}/fhir/$fhirpath"),
                &body,
                parse_fhirpath,
            )
            .await
        }
        "sql" => {
            let body = json!({ "query": interpolate(&src_str, scope),
                "params": cell.config.get("params").cloned().unwrap_or_else(|| json!([])) });
            post_parse(client, &format!("{base}/api/$sql"), &body, parse_sql).await
        }
        "sql-on-fhir" => {
            let limit = cell
                .config
                .get("limit")
                .and_then(|l| l.as_i64())
                .unwrap_or(100);
            let body = json!({ "resourceType": "Parameters", "parameter": [
                { "name": "viewDefinition", "resource": deep_interp(&cell.source, scope) },
                { "name": "limit", "valueInteger": limit } ] });
            post_parse(
                client,
                &format!("{base}/fhir/ViewDefinition/$run"),
                &body,
                parse_sof,
            )
            .await
        }
        "cql" => {
            let src = interpolate(&src_str, scope);
            let is_lib = src.lines().any(|l| {
                let t = l.trim_start();
                CQL_LIB.iter().any(|kw| t.starts_with(kw))
            });
            let mut parameter = vec![json!({
                "name": if is_lib { "library" } else { "expression" }, "valueString": src })];
            if let Some(ctx) = cell.config.get("context").and_then(|c| c.as_str()) {
                parameter.push(json!({ "name": "context", "valueString": ctx }));
            }
            let body = json!({ "resourceType": "Parameters", "parameter": parameter });
            post_parse(client, &format!("{base}/fhir/$cql"), &body, parse_cql).await
        }
        "graphql" => {
            let body = json!({ "query": interpolate(&src_str, scope),
                "variables": deep_interp(&cell.config.get("variables").cloned().unwrap_or_else(|| json!({})), scope) });
            match client
                .post(format!("{base}/$graphql"))
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(j) => {
                        if let Some(errs) = j
                            .get("errors")
                            .and_then(|e| e.as_array())
                            .filter(|a| !a.is_empty())
                        {
                            let msg = errs
                                .iter()
                                .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                                .collect::<Vec<_>>()
                                .join("; ");
                            err_out(msg)
                        } else {
                            out(
                                "json",
                                json!({ "data": j.get("data").cloned().unwrap_or(Value::Null) }),
                            )
                        }
                    }
                    Err(e) => err_out(e.to_string()),
                },
                Err(e) => err_out(e.to_string()),
            }
        }
        "rest" => {
            let method = cell
                .source
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("GET")
                .to_uppercase();
            let raw_url = interpolate(
                cell.source
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or(""),
                scope,
            );
            let url = if raw_url.starts_with("http") {
                raw_url
            } else if raw_url.starts_with('/') {
                format!("{base}/fhir{raw_url}")
            } else {
                format!("{base}/fhir/{raw_url}")
            };
            let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
            let mut req = client.request(m, &url);
            if method != "GET"
                && let Some(b) = cell.source.get("body").filter(|b| !b.is_null())
            {
                req = req.json(&deep_interp(b, scope));
            }
            match req.send().await {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(j) => {
                        if j.get("resourceType").is_some() {
                            out("bundle", json!({ "data": j }))
                        } else {
                            out("json", json!({ "data": j }))
                        }
                    }
                    Err(e) => err_out(e.to_string()),
                },
                Err(e) => err_out(e.to_string()),
            }
        }
        // not executed headless — keep whatever is cached
        _ => out("json", json!({ "data": Value::Null })),
    }
}

async fn post_parse(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
    parse: fn(&Value) -> Output,
) -> Output {
    match client.post(url).json(body).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.json::<Value>().await {
                Ok(j) => {
                    if status.is_success() {
                        parse(&j)
                    } else {
                        err_out(format!("HTTP {status}"))
                    }
                }
                Err(e) => err_out(e.to_string()),
            }
        }
        Err(e) => err_out(e.to_string()),
    }
}

/// Insert a named cell's output into the scope (table/value/json shapes).
fn scope_insert(scope: &mut Map<String, Value>, name: &str, o: &Output) {
    let v = match o.kind.as_str() {
        "table" => json!({
            "columns": o.rest.get("columns").cloned().unwrap_or_else(|| json!([])),
            "rows": o.rest.get("rows").cloned().unwrap_or_else(|| json!([])),
        }),
        "value" => o.rest.get("data").cloned().unwrap_or_else(|| json!([])),
        "json" | "bundle" => o.rest.get("data").cloned().unwrap_or(Value::Null),
        _ => return,
    };
    scope.insert(name.to_string(), v);
}

// ─────────────────────────── handler ───────────────────────────

/// `POST /api/notebooks/{id}/run` — execute the whole notebook headless and return
/// it with fresh outputs. Does not persist (the caller decides whether to save).
pub async fn notebook_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let stored = state
        .storage
        .read_raw("Notebook", &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Notebook/{id} not found")))?;
    let doc: Value = serde_json::from_str(&stored.resource_json)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut nb: Notebook =
        octofhir_notebook::parse(&doc).map_err(|e| ApiError::internal(e.to_string()))?;

    // self-call the local server; forward the caller's auth
    let base = format!("http://127.0.0.1:{}", state.config.server.port);
    let mut cb = reqwest::Client::builder();
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        let mut h = reqwest::header::HeaderMap::new();
        if let Ok(hv) = reqwest::header::HeaderValue::from_bytes(auth.as_bytes()) {
            h.insert(reqwest::header::AUTHORIZATION, hv);
        }
        cb = cb.default_headers(h);
    }
    let client = cb.build().map_err(|e| ApiError::internal(e.to_string()))?;

    // seed scope with notebook variables
    let mut scope: Map<String, Value> = Map::new();
    for v in &nb.variables {
        if let (Some(name), Some(val)) = (v.get("name").and_then(|n| n.as_str()), v.get("value")) {
            scope.insert(name.to_string(), val.clone());
        }
    }

    for idx in topo_order(&nb) {
        let output = {
            let cell = &nb.cells[idx];
            if matches!(cell.cell_type.as_str(), "pipeline" | "chart" | "input") {
                continue; // not executed headless
            }
            run_cell(&client, &base, cell, &scope).await
        };
        if let Some(name) = nb.cells[idx].name.clone() {
            scope_insert(&mut scope, &name, &output);
        }
        let cell = &mut nb.cells[idx];
        cell.exec_count = Some(cell.exec_count.unwrap_or(0) + 1);
        cell.outputs = vec![output];
    }

    let out = serde_json::to_value(&nb).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(out).into_response())
}
