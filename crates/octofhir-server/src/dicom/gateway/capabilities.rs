//! DICOM Retrieve Capabilities using the WADL resource tree from PS3.18 Annex H.
//!
//! # Contents
//! Authenticated base OPTIONS and a stateless pagination status report.
//! # Invariants
//! Only implemented routes are advertised. Published URLs contain no PACS origins,
//! credentials or patient data. CORS preflight remains a separate transaction.
use super::{encoding::escape, missing, negotiation::Preferences, query};
use crate::{operations::OperationContext, server::AppState};
use axum::{
    Json,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use octofhir_api::ApiError;
use std::collections::BTreeMap;

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    media: Vec<&'static str>,
}
impl Node {
    fn insert(&mut self, path: &str, media: &[&'static str]) {
        let mut node = self;
        for part in path.split('/') {
            node = node.children.entry(part.into()).or_default();
        }
        node.media = media.to_vec();
    }
    fn xml(&self, out: &mut String) {
        for (path, node) in &self.children {
            out.push_str(&format!("<resource path=\"{}\">", escape(path)));
            if !node.media.is_empty() {
                out.push_str("<method name=\"GET\"><request><param name=\"Accept\" style=\"header\" required=\"true\" type=\"xs:string\"/><param name=\"accept\" style=\"query\" type=\"xs:string\"/><param name=\"charset\" style=\"query\" type=\"xs:string\"/></request><response status=\"200\">");
                for media in &node.media {
                    out.push_str(&format!(
                        "<representation mediaType=\"{}\"/>",
                        escape(media)
                    ));
                }
                out.push_str("</response></method>");
            }
            node.xml(out);
            out.push_str("</resource>");
        }
    }
}
fn tree() -> Node {
    let mut tree = Node::default();
    let metadata = [super::encoding::JSON, super::encoding::XML];
    let object = [
        "multipart/related; type=\"application/dicom\"",
        "multipart/related; type=\"application/octet-stream\"",
    ];
    let rendered = [
        "multipart/related; type=\"image/jpeg\"",
        "multipart/related; type=\"image/png\"",
    ];
    let study = "studies/{study}";
    let series = "studies/{study}/series/{series}";
    let instance = "studies/{study}/series/{series}/instances/{instance}";
    for path in [
        "studies",
        "series",
        "instances",
        "studies/{study}/series",
        "studies/{study}/instances",
        "studies/{study}/series/{series}/instances",
    ] {
        tree.insert(path, &metadata);
    }
    for path in [study, series, instance] {
        tree.insert(path, &object);
        tree.insert(&format!("{path}/metadata"), &metadata);
        tree.insert(&format!("{path}/rendered"), &rendered);
        tree.insert(&format!("{path}/thumbnail"), &["image/jpeg", "image/png"]);
        tree.insert(
            &format!("{path}/pixeldata"),
            &["multipart/related; type=\"application/octet-stream\""],
        );
    }
    tree.insert(
        &format!("{instance}/frames/{{frames}}"),
        &["multipart/related; type=\"application/octet-stream\""],
    );
    tree.insert(&format!("{instance}/frames/{{frames}}/rendered"), &rendered);
    tree.insert(
        &format!("{instance}/frames/{{frames}}/thumbnail"),
        &["image/jpeg", "image/png"],
    );
    tree.insert(
        &format!("{instance}/bulkdata/{{token}}"),
        &["multipart/related; type=\"application/octet-stream\""],
    );
    tree
}
fn admission(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
) -> Result<(), ApiError> {
    if context.auth().is_none() {
        return Err(ApiError::forbidden(
            "DICOMweb requires an authenticated FHIR identity",
        ));
    }
    state
        .config
        .dicom
        .connections
        .get(connection)
        .and_then(|c| c.gateway.as_ref())
        .ok_or_else(missing)?;
    Ok(())
}
pub(super) async fn options(
    State(state): State<AppState>,
    context: OperationContext,
    Path(connection): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    admission(&state, &context, &connection)?;
    let mut query = query::parse(uri.query())?;
    if !query.is_empty() {
        return Err(ApiError::bad_request(
            "DICOM capabilities has no query parameters",
        ));
    }
    let preferences = Preferences::new(&headers, &mut query)?;
    preferences.select(&["application/vnd.sun.wadl+xml"])?;
    let base = format!(
        "{}/dicomweb/{connection}/",
        state.base_url.trim_end_matches('/')
    );
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><application xmlns=\"http://wadl.dev.java.net/2009/02\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><doc title=\"OctoFHIR DICOMweb gateway\">Authenticated, attested manifests only. Metadata encoding: UTF-8. Renderings and transfer syntaxes depend on the configured PACS. STOW and UPS origin-server operations are not provided. QIDO scan limits apply.</doc><resources base=\"{}\">",
        escape(&base)
    );
    tree().xml(&mut xml);
    xml.push_str("</resources></application>");
    let mut response = xml.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sun.wadl+xml; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("OPTIONS"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Accept"),
    );
    Ok(response)
}
pub(super) async fn status(
    State(state): State<AppState>,
    context: OperationContext,
    Path(connection): Path<String>,
) -> Result<Response, ApiError> {
    admission(&state, &context, &connection)?;
    let mut response=Json(serde_json::json!({"status":"additional-results","message":"Additional authorized results exist. Follow the response Link with rel=next, or advance offset by limit. Results are live and are not a snapshot cursor."})).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advertised_tree_has_all_search_and_thumbnail_levels_without_store() {
        let mut xml = String::new();
        tree().xml(&mut xml);
        assert_eq!(xml.matches("path=\"thumbnail\"").count(), 4);
        assert!(!xml.contains("POST"));
        assert!(xml.contains("path=\"instances\""));
        assert!(xml.contains("application/dicom+xml"));
    }
}
