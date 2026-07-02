//! Import/export between `.fhirnb` and Jupyter `.ipynb`, FHIR Bundle, Markdown,
//! and HTML. See docs/ui-notebooks-plan.md §10.

use crate::notebook::{Cell, Notebook, Output};
use crate::NotebookError;
use serde_json::{json, Map, Value};

/// Cell types whose `source` is an object (not a string).
fn is_object_source(cell_type: &str) -> bool {
    matches!(
        cell_type,
        "sql-on-fhir" | "rest" | "chart" | "pipeline" | "input"
    )
}

/// Jupyter magic token ↔ our cell type. `sof` aliases `sql-on-fhir`.
fn magic_for(cell_type: &str) -> &str {
    match cell_type {
        "sql-on-fhir" => "sof",
        other => other,
    }
}
fn type_for_magic(magic: &str) -> String {
    match magic {
        "sof" => "sql-on-fhir".to_string(),
        other => other.to_string(),
    }
}

fn source_to_string(cell: &Cell) -> String {
    match &cell.source {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    }
}

/// Split a string into ipynb `source` lines (each keeps its trailing `\n` except
/// the last), matching nbformat convention.
fn to_source_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    text.split_inclusive('\n').map(|s| s.to_string()).collect()
}

fn join_source(source: &Value) -> String {
    match source {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<String>(),
        _ => String::new(),
    }
}

// ─────────────────────────── ipynb ───────────────────────────

/// Export a Notebook to Jupyter nbformat v4 JSON.
pub fn to_ipynb(nb: &Notebook) -> Value {
    let cells: Vec<Value> = nb
        .cells
        .iter()
        .map(|c| {
            if c.cell_type == "markdown" {
                json!({
                    "cell_type": "markdown",
                    "metadata": {},
                    "source": to_source_lines(&source_to_string(c)),
                })
            } else {
                let body = format!("%%{}\n{}", magic_for(&c.cell_type), source_to_string(c));
                let mut meta = Map::new();
                let mut octo = Map::new();
                octo.insert("type".into(), json!(c.cell_type));
                if let Some(n) = &c.name {
                    octo.insert("name".into(), json!(n));
                }
                if !c.config.is_null() {
                    octo.insert("config".into(), c.config.clone());
                }
                meta.insert("octofhir".into(), Value::Object(octo));
                json!({
                    "cell_type": "code",
                    "execution_count": c.exec_count,
                    "metadata": meta,
                    "source": to_source_lines(&body),
                    "outputs": outputs_to_ipynb(&c.outputs),
                })
            }
        })
        .collect();

    json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {
            "octofhir": {
                "nbformat": nb.nbformat,
                "title": nb.title,
                "description": nb.description,
                "fhirVersion": nb.fhir_version,
                "tags": nb.tags,
                "variables": nb.variables,
                "defaults": nb.defaults,
            }
        },
        "cells": cells,
    })
}

fn outputs_to_ipynb(outputs: &[Output]) -> Vec<Value> {
    outputs
        .iter()
        .map(|o| {
            let mut payload = o.rest.clone();
            payload.insert("kind".into(), json!(o.kind));
            json!({
                "output_type": "execute_result",
                "execution_count": Value::Null,
                "metadata": {},
                "data": { "application/json": Value::Object(payload) },
            })
        })
        .collect()
}

/// Import a Jupyter notebook into a `.fhirnb` Notebook (best-effort).
pub fn from_ipynb(doc: &Value) -> Result<Notebook, NotebookError> {
    let octo = doc.get("metadata").and_then(|m| m.get("octofhir"));
    let title = octo
        .and_then(|o| o.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("Imported notebook")
        .to_string();

    let cells_in = doc
        .get("cells")
        .and_then(|c| c.as_array())
        .ok_or_else(|| NotebookError::Format("ipynb has no cells array".into()))?;

    let mut cells = Vec::new();
    for (i, jc) in cells_in.iter().enumerate() {
        let id = format!("c{}", i + 1);
        let kind = jc.get("cell_type").and_then(|v| v.as_str()).unwrap_or("");
        let raw = join_source(jc.get("source").unwrap_or(&Value::Null));
        if kind == "markdown" {
            cells.push(Cell {
                id,
                cell_type: "markdown".into(),
                name: None,
                collapsed: None,
                source: json!(raw),
                config: Value::Null,
                outputs: vec![],
                exec_count: None,
            });
            continue;
        }
        // code cell: parse magic header line
        let (magic, body) = split_magic(&raw);
        let co = jc.get("metadata").and_then(|m| m.get("octofhir"));
        let cell_type = co
            .and_then(|o| o.get("type"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| type_for_magic(&magic));
        let source = if is_object_source(&cell_type) {
            serde_json::from_str::<Value>(body.trim()).unwrap_or(Value::Null)
        } else {
            json!(body)
        };
        cells.push(Cell {
            id,
            cell_type,
            name: co
                .and_then(|o| o.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            collapsed: None,
            source,
            config: co
                .and_then(|o| o.get("config"))
                .cloned()
                .unwrap_or(Value::Null),
            outputs: vec![],
            exec_count: None,
        });
    }

    Ok(Notebook {
        resource_type: "Notebook".into(),
        id: None,
        meta: None,
        nbformat: 1,
        title,
        description: octo
            .and_then(|o| o.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        fhir_version: octo
            .and_then(|o| o.get("fhirVersion"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tags: octo
            .and_then(|o| o.get("tags"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        defaults: octo
            .and_then(|o| o.get("defaults"))
            .cloned()
            .unwrap_or(Value::Null),
        variables: octo
            .and_then(|o| o.get("variables"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        cells,
    })
}

/// Strip a leading `%%magic` line, returning (magic, body).
fn split_magic(raw: &str) -> (String, String) {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("%%") {
        let nl = rest.find('\n').unwrap_or(rest.len());
        let magic = rest[..nl].trim().to_string();
        let body = rest.get(nl + 1..).unwrap_or("").to_string();
        (magic, body)
    } else {
        (String::new(), raw.to_string())
    }
}

// ─────────────────────────── Bundle ───────────────────────────

/// Wrap a Notebook in a FHIR collection Bundle.
pub fn to_bundle(nb: &Notebook) -> Result<Value, NotebookError> {
    let resource = serde_json::to_value(nb)?;
    Ok(json!({
        "resourceType": "Bundle",
        "type": "collection",
        "entry": [ { "resource": resource } ],
    }))
}

/// Extract the first `Notebook` resource from a Bundle (or accept a bare Notebook).
pub fn from_bundle(doc: &Value) -> Result<Notebook, NotebookError> {
    if doc.get("resourceType").and_then(|v| v.as_str()) == Some("Notebook") {
        return Ok(serde_json::from_value(doc.clone())?);
    }
    let entries = doc
        .get("entry")
        .and_then(|e| e.as_array())
        .ok_or_else(|| NotebookError::Format("Bundle has no entry array".into()))?;
    for e in entries {
        if let Some(res) = e.get("resource") {
            if res.get("resourceType").and_then(|v| v.as_str()) == Some("Notebook") {
                return Ok(serde_json::from_value(res.clone())?);
            }
        }
    }
    Err(NotebookError::Format(
        "no Notebook resource in Bundle".into(),
    ))
}

// ─────────────────────────── Markdown ───────────────────────────

fn table_to_md(o: &Output) -> Option<String> {
    let cols = o.rest.get("columns")?.as_array()?;
    let rows = o.rest.get("rows")?.as_array()?;
    let header: Vec<String> = cols.iter().map(cell_text).collect();
    let mut out = String::new();
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!("| {} |\n", vec!["---"; header.len()].join(" | ")));
    for row in rows.iter().take(100) {
        if let Some(arr) = row.as_array() {
            let cells: Vec<String> = arr.iter().map(cell_text).collect();
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
        }
    }
    Some(out)
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Render a Notebook to static Markdown (prose + fenced code + result tables).
pub fn to_markdown(nb: &Notebook) -> String {
    let mut out = format!("# {}\n\n", nb.title);
    if let Some(d) = &nb.description {
        out.push_str(&format!("{d}\n\n"));
    }
    for c in &nb.cells {
        if c.cell_type == "markdown" {
            out.push_str(&source_to_string(c));
            out.push_str("\n\n");
            continue;
        }
        out.push_str(&format!(
            "```{}\n{}\n```\n\n",
            c.cell_type,
            source_to_string(c)
        ));
        for o in &c.outputs {
            match o.kind.as_str() {
                "table" => {
                    if let Some(md) = table_to_md(o) {
                        out.push_str(&md);
                        out.push('\n');
                    }
                }
                "error" => {
                    let msg = o
                        .rest
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("error");
                    out.push_str(&format!("> ⚠️ {msg}\n\n"));
                }
                _ => {
                    if let Some(data) = o.rest.get("data").or_else(|| o.rest.get("text")) {
                        out.push_str(&format!(
                            "```json\n{}\n```\n\n",
                            serde_json::to_string_pretty(data).unwrap_or_default()
                        ));
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────── HTML ───────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn table_to_html(o: &Output) -> Option<String> {
    let cols = o.rest.get("columns")?.as_array()?;
    let rows = o.rest.get("rows")?.as_array()?;
    let mut html = String::from("<table><thead><tr>");
    for c in cols {
        html.push_str(&format!("<th>{}</th>", esc(&cell_text(c))));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows.iter().take(500) {
        if let Some(arr) = row.as_array() {
            html.push_str("<tr>");
            for v in arr {
                html.push_str(&format!("<td>{}</td>", esc(&cell_text(v))));
            }
            html.push_str("</tr>");
        }
    }
    html.push_str("</tbody></table>");
    Some(html)
}

/// Render a Notebook to a static, self-contained HTML report.
pub fn to_html(nb: &Notebook) -> String {
    let mut body = format!("<h1>{}</h1>", esc(&nb.title));
    if let Some(d) = &nb.description {
        body.push_str(&format!("<p class=desc>{}</p>", esc(d)));
    }
    for c in &nb.cells {
        body.push_str("<section class=cell>");
        if c.cell_type == "markdown" {
            body.push_str(&format!(
                "<pre class=md>{}</pre>",
                esc(&source_to_string(c))
            ));
        } else {
            body.push_str(&format!(
                "<div class=label>{}</div><pre class=code>{}</pre>",
                esc(&c.cell_type),
                esc(&source_to_string(c))
            ));
            for o in &c.outputs {
                match o.kind.as_str() {
                    "table" => {
                        if let Some(h) = table_to_html(o) {
                            body.push_str(&h);
                        }
                    }
                    "error" => {
                        let msg = o
                            .rest
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("error");
                        body.push_str(&format!("<pre class=err>{}</pre>", esc(msg)));
                    }
                    _ => {
                        if let Some(data) = o.rest.get("data").or_else(|| o.rest.get("text")) {
                            body.push_str(&format!(
                                "<pre class=out>{}</pre>",
                                esc(&serde_json::to_string_pretty(data).unwrap_or_default())
                            ));
                        }
                    }
                }
            }
        }
        body.push_str("</section>");
    }

    format!(
        "<!doctype html><html><head><meta charset=utf-8><title>{}</title><style>\
body{{font-family:system-ui,sans-serif;max-width:900px;margin:2rem auto;padding:0 1rem;color:#1e293b}}\
h1{{font-weight:680}}.desc{{color:#64748b}}.cell{{margin:1.25rem 0;border:1px solid #e2e8f0;border-radius:12px;padding:1rem}}\
.label{{font-size:11px;text-transform:uppercase;letter-spacing:.05em;color:#6366f1;font-weight:600;margin-bottom:.4rem}}\
pre{{background:#f8fafc;border-radius:8px;padding:.6rem .8rem;overflow:auto;font-size:13px}}\
.err{{background:#fef2f2;color:#dc2626}}table{{border-collapse:collapse;width:100%;font-size:13px}}\
th,td{{border:1px solid #e2e8f0;padding:4px 8px;text-align:left}}th{{background:#f1f5f9}}\
</style></head><body>{}</body></html>",
        esc(&nb.title),
        body
    )
}
