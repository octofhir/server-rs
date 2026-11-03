//! Console API for the FML editor: check as you type, format, and compile to the StructureMap
//! the server stores. Nothing here reads or writes data.
//!
//! - `POST /api/fml/check` `{ "source": "…" }` → syntax, compile and type-check problems, with
//!   1-based lines and UTF-16 columns (the editor's coordinates);
//! - `POST /api/fml/format` `{ "source": "…" }` → `{ "source": "…" }`, or 422 with the syntax
//!   problems;
//! - `POST /api/fml/compile` `{ "source": "…" }` → the StructureMap in the server's FHIR release
//!   (R4 form on R4 and R4B), or 422 with the problems;
//! - `POST /api/fml/print` a StructureMap (R4 or R5) → `{ "source": "…" }`, its FML.

use std::sync::{Arc, OnceLock};

use axum::Json;
use axum::extract::State;
use fml_check::{CoreDefinitions, Release};
use fml_syntax::{Diagnostic, FormatError, FormatOptions, LineIndex, Severity};
use octofhir_api::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::server::AppState;

#[derive(Deserialize)]
pub struct SourceBody {
    source: String,
}

/// A problem at a range of the source.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    severity: &'static str,
    message: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

fn problems(text: &str, diagnostics: &[Diagnostic]) -> Vec<Problem> {
    let index = LineIndex::new(text);
    diagnostics
        .iter()
        .map(|diagnostic| {
            let start = index.line_col(text, diagnostic.range.start());
            let end = index.line_col(text, diagnostic.range.end());
            Problem {
                severity: match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                message: diagnostic.message.clone(),
                start_line: start.line + 1,
                start_column: start.col_utf16 + 1,
                end_line: end.line + 1,
                end_column: end.col_utf16 + 1,
            }
        })
        .collect()
}

/// The core definitions of the server's release, built once.
fn core(state: &AppState) -> Option<(Release, Arc<CoreDefinitions>)> {
    static CORE: OnceLock<Option<(Release, Arc<CoreDefinitions>)>> = OnceLock::new();
    CORE.get_or_init(|| {
        Release::parse(&state.fhir_version)
            .map(|release| (release, Arc::new(CoreDefinitions::new(release))))
    })
    .clone()
}

/// `POST /api/fml/check`
pub async fn check(
    State(state): State<AppState>,
    Json(body): Json<SourceBody>,
) -> Result<Json<Value>, ApiError> {
    let core = core(&state);
    let result = tokio::task::spawn_blocking(move || {
        let text = body.source;
        let parse = fml_syntax::parse(&text);
        let compiled = fml_model::compile_tree(&parse.tree());
        let mut diagnostics = parse.diagnostics().to_vec();
        diagnostics.extend(compiled.diagnostics);
        if !diagnostics.iter().any(|d| d.severity == Severity::Error)
            && let Some((_, core)) = &core
        {
            diagnostics.extend(fml_check::check(&parse.tree(), core.as_ref()));
        }
        diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));
        json!({
            "url": compiled.map.url,
            "name": compiled.map.name,
            "problems": problems(&text, &diagnostics),
        })
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(result))
}

/// `POST /api/fml/format`
pub async fn format(Json(body): Json<SourceBody>) -> Result<Json<Value>, ApiError> {
    match fml_syntax::format(&body.source, &FormatOptions::default()) {
        Ok(source) => Ok(Json(json!({ "source": source }))),
        Err(FormatError::Syntax(diagnostics)) => Err(ApiError::unprocessable_entity(
            "The map has syntax errors",
            Some(json!({ "problems": problems(&body.source, &diagnostics) })),
        )),
        Err(FormatError::Unstable(reason)) => Err(ApiError::internal(format!(
            "The formatter could not keep the map unchanged: {reason}"
        ))),
    }
}

/// `POST /api/fml/compile`
pub async fn compile(
    State(state): State<AppState>,
    Json(body): Json<SourceBody>,
) -> Result<Json<Value>, ApiError> {
    let compiled = fml_model::compile(&body.source);
    if compiled.has_errors() {
        return Err(ApiError::unprocessable_entity(
            "The map does not compile",
            Some(json!({ "problems": problems(&body.source, &compiled.diagnostics) })),
        ));
    }
    let r4 = matches!(
        Release::parse(&state.fhir_version),
        Some(Release::R4 | Release::R4B)
    );
    let map = if r4 {
        fml_model::to_r4(&compiled.map).json
    } else {
        serde_json::to_value(&compiled.map)
            .map_err(|error| ApiError::internal(error.to_string()))?
    };
    Ok(Json(map))
}

/// `POST /api/fml/print`
pub async fn print(Json(map): Json<Value>) -> Result<Json<Value>, ApiError> {
    if map["resourceType"] != "StructureMap" {
        return Err(ApiError::bad_request("Expected a StructureMap"));
    }
    let map = fml_model::from_json(map)
        .map_err(|error| ApiError::bad_request(format!("Not a valid StructureMap: {error}")))?;
    Ok(Json(json!({ "source": fml_model::print(&map) })))
}
