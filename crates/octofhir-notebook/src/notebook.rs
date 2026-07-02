//! `.fhirnb` v1 document model. Cells are open content (the frontend owns the
//! per-type schema), so `source`/`config` stay as `serde_json::Value` — the model
//! captures structure enough for validation + import/export. See
//! docs/ui-notebooks-spec.md §1, §4.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notebook {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    pub nbformat: u8,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "fhirVersion", skip_serializing_if = "Option::is_none")]
    pub fhir_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub defaults: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Value>,
    #[serde(default)]
    pub cells: Vec<Cell>,
}

/// Flat cell model — `type` discriminates; `source`/`config` are open JSON so any
/// cell kind round-trips losslessly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub id: String,
    #[serde(rename = "type")]
    pub cell_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub source: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub config: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Output>,
    #[serde(rename = "execCount", skip_serializing_if = "Option::is_none")]
    pub exec_count: Option<u32>,
}

/// Output — `kind` discriminates; remaining fields (columns/rows/data/text/…)
/// are flattened so any output kind is preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub kind: String,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl Notebook {
    pub fn is_notebook(&self) -> bool {
        self.resource_type == "Notebook"
    }

    /// Source as a plain string (string cells) or `None` for object-source cells.
    pub fn cell_source_str(cell: &Cell) -> Option<&str> {
        cell.source.as_str()
    }
}
