//! OctoFHIR Notebook (`.fhirnb`) model + import/export converters.
//!
//! The document model ([`Notebook`]) mirrors `docs/ui-notebooks-spec.md`. Cell
//! contents are open JSON (the frontend owns per-type schemas), so this crate
//! focuses on the stable envelope + lossless conversion to/from Jupyter `.ipynb`,
//! FHIR Bundle, Markdown, and HTML. Headless cell execution is intentionally out
//! of scope here (it belongs with the server's engine handlers).

pub mod convert;
pub mod notebook;

pub use notebook::{Cell, Notebook, Output};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotebookError {
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("format error: {0}")]
    Format(String),
}

/// Export formats supported by [`export`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Fhirnb,
    Ipynb,
    Bundle,
    Markdown,
    Html,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "fhirnb" | "json" => Some(Format::Fhirnb),
            "ipynb" => Some(Format::Ipynb),
            "bundle" => Some(Format::Bundle),
            "markdown" | "md" => Some(Format::Markdown),
            "html" => Some(Format::Html),
            _ => None,
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Format::Fhirnb | Format::Bundle => "application/fhir+json",
            Format::Ipynb => "application/json",
            Format::Markdown => "text/markdown; charset=utf-8",
            Format::Html => "text/html; charset=utf-8",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Fhirnb => "fhirnb.json",
            Format::Ipynb => "ipynb",
            Format::Bundle => "bundle.json",
            Format::Markdown => "md",
            Format::Html => "html",
        }
    }
}

/// Parse a `.fhirnb` document from JSON.
pub fn parse(doc: &serde_json::Value) -> Result<Notebook, NotebookError> {
    Ok(serde_json::from_value(doc.clone())?)
}

/// Serialize a Notebook in the requested format. Returns raw bytes as a String.
pub fn export(nb: &Notebook, format: Format) -> Result<String, NotebookError> {
    Ok(match format {
        Format::Fhirnb => serde_json::to_string_pretty(nb)?,
        Format::Ipynb => serde_json::to_string_pretty(&convert::to_ipynb(nb))?,
        Format::Bundle => serde_json::to_string_pretty(&convert::to_bundle(nb)?)?,
        Format::Markdown => convert::to_markdown(nb),
        Format::Html => convert::to_html(nb),
    })
}

/// Import a document (JSON) in the given format into a Notebook. Markdown/HTML are
/// export-only, so importing them is a [`NotebookError::Format`].
pub fn import(doc: &serde_json::Value, format: Format) -> Result<Notebook, NotebookError> {
    match format {
        Format::Fhirnb => parse(doc),
        Format::Ipynb => convert::from_ipynb(doc),
        Format::Bundle => convert::from_bundle(doc),
        Format::Markdown | Format::Html => Err(NotebookError::Format(
            "markdown/html import is not supported".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Notebook {
        parse(&json!({
            "resourceType": "Notebook",
            "nbformat": 1,
            "title": "Demo",
            "fhirVersion": "R4",
            "tags": ["t"],
            "cells": [
                { "id": "c1", "type": "markdown", "source": "# Hello\nworld" },
                { "id": "c2", "type": "sql", "name": "q",
                  "source": "SELECT 1 AS n",
                  "outputs": [ { "kind": "table", "columns": ["n"], "rows": [[1]],
                                 "meta": { "rowCount": 1, "truncated": false } } ] },
                { "id": "c3", "type": "sql-on-fhir",
                  "source": { "resourceType": "ViewDefinition", "resource": "Patient" } }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn ipynb_round_trip_preserves_cells() {
        let nb = sample();
        let ipynb = convert::to_ipynb(&nb);
        let back = convert::from_ipynb(&ipynb).unwrap();
        assert_eq!(back.title, "Demo");
        assert_eq!(back.cells.len(), 3);
        assert_eq!(back.cells[0].cell_type, "markdown");
        assert_eq!(back.cells[1].cell_type, "sql");
        assert_eq!(back.cells[1].name.as_deref(), Some("q"));
        assert_eq!(back.cells[2].cell_type, "sql-on-fhir");
        // object source survives the JSON encode/parse
        assert_eq!(
            back.cells[2]
                .source
                .get("resource")
                .and_then(|v| v.as_str()),
            Some("Patient")
        );
    }

    #[test]
    fn bundle_round_trip() {
        let nb = sample();
        let bundle = convert::to_bundle(&nb).unwrap();
        assert_eq!(bundle["resourceType"], "Bundle");
        let back = convert::from_bundle(&bundle).unwrap();
        assert_eq!(back.title, "Demo");
        assert_eq!(back.cells.len(), 3);
    }

    #[test]
    fn markdown_export_has_table() {
        let md = convert::to_markdown(&sample());
        assert!(md.contains("# Demo"));
        assert!(md.contains("```sql"));
        assert!(md.contains("| n |"));
    }

    #[test]
    fn html_export_is_self_contained() {
        let html = convert::to_html(&sample());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<h1>Demo</h1>"));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn format_parse() {
        assert_eq!(Format::parse("ipynb"), Some(Format::Ipynb));
        assert_eq!(Format::parse("MD"), Some(Format::Markdown));
        assert_eq!(Format::parse("nope"), None);
    }
}
