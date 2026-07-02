//! Notebook import/export endpoints. CRUD is served by the generic FHIR routes
//! (`/fhir/Notebook`); these add format conversion on top. See
//! docs/ui-notebooks-plan.md §7b.

use crate::server::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use octofhir_api::ApiError;
use octofhir_notebook::{self as nb, Format};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct FormatQuery {
    #[serde(default)]
    format: Option<String>,
}

fn resolve_format(q: &Option<String>, default: Format) -> Result<Format, ApiError> {
    match q {
        Some(s) => {
            Format::parse(s).ok_or_else(|| ApiError::bad_request(format!("unknown format: {s}")))
        }
        None => Ok(default),
    }
}

/// `POST /api/notebooks/import?format=fhirnb|ipynb|bundle` — convert an uploaded
/// document into a `.fhirnb` Notebook (not persisted; the client saves it via CRUD).
pub async fn notebook_import(
    Query(q): Query<FormatQuery>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let format = resolve_format(&q.format, Format::Fhirnb)?;
    let notebook = nb::import(&body, format).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let out = serde_json::to_value(&notebook).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((StatusCode::OK, Json(out)).into_response())
}

/// `GET /api/notebooks/{id}/export?format=fhirnb|ipynb|bundle|markdown|html` — read
/// the stored Notebook and serialize it in the requested format.
pub async fn notebook_export(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<FormatQuery>,
) -> Result<Response, ApiError> {
    let format = resolve_format(&q.format, Format::Fhirnb)?;
    let stored = state
        .storage
        .read_raw("Notebook", &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Notebook/{id} not found")))?;
    let doc: Value = serde_json::from_str(&stored.resource_json)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let notebook = nb::parse(&doc).map_err(|e| ApiError::internal(e.to_string()))?;
    let body = nb::export(&notebook, format).map_err(|e| ApiError::internal(e.to_string()))?;

    let safe_title: String = notebook
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let filename = format!("{safe_title}.{}", format.extension());

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, format.content_type().to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}
