//! `$ccda-to-fhir` and `$fhir-to-ccda`: C-CDA documents to FHIR document Bundles and back, with
//! the FML maps of [ccda-fhir](https://github.com/octofhir/ccda-fhir).
//!
//! The API stays JSON: a CDA document travels as a `Binary` (`contentType: application/xml`,
//! `data` in base64), in a `Parameters` parameter or as the request body.
//!
//! - `POST /fhir/$ccda-to-fhir` → `Parameters` with `return` (the document Bundle) and `outcome`
//!   (an OperationOutcome listing what the conversion could not carry over: elements outside the
//!   CDA model, templates the maps do not know, codes without a translation; with the XML line).
//! - `POST /fhir/$fhir-to-ccda` with a document Bundle or a Composition (as `resource` or the
//!   body) → `Parameters` with `return` (a `Binary` holding the CDA document) and `outcome`.
//!
//! `$convert` is not involved: it converts between FHIR versions.
//!
//! The console's FML page runs maps over CDA documents through `POST /api/fml/cda-run`
//! (`{ "source": FML, "xml": "…", "debug": true }`): the document read into the CDA model, the
//! map run with the converter's definitions and maps, the debug trace as `$transform?debug`
//! gives it, and where each input path is in the XML.

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use ccda_fhir::{Converter, Options};
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::operations::handler::OperationLevel;
use crate::operations::{
    OperationContext, OperationDefinition, OperationError, OperationHandler, OperationKind,
    OperationRegistry,
};
use crate::server::AppState;

/// The largest CDA document accepted, decoded.
const MAX_DOCUMENT_BYTES: usize = 32 * 1024 * 1024;

/// The converter, loaded on first use and shared: its maps, models and ConceptMaps are embedded.
#[derive(Clone, Default)]
pub struct CcdaConverter {
    converter: Arc<OnceCell<Converter>>,
}

impl CcdaConverter {
    /// Runs `work` with the converter on a blocking thread: the engine's futures borrow
    /// definitions and maps that are not `Sync`, so they run there, as `$transform` does.
    async fn with<T, F>(&self, work: F) -> Result<T, OperationError>
    where
        T: Send + 'static,
        F: for<'c> FnOnce(
                &'c Converter,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, OperationError>> + 'c>,
            > + Send
            + 'static,
    {
        let cell = self.converter.clone();
        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            handle.block_on(async move {
                let converter = cell
                    .get_or_try_init(|| async { Converter::new().await })
                    .await
                    .map_err(|e| OperationError::Internal(format!("C-CDA converter: {e}")))?;
                work(converter).await
            })
        })
        .await
        .map_err(|e| OperationError::Internal(format!("C-CDA conversion: {e}")))?
    }
}

/// Handler for `$ccda-to-fhir`.
pub struct CcdaToFhirOperation {
    converter: CcdaConverter,
}

/// Handler for `$fhir-to-ccda`.
pub struct FhirToCcdaOperation {
    converter: CcdaConverter,
}

/// The converter of the process: the operations and the console share it.
static SHARED: std::sync::LazyLock<CcdaConverter> =
    std::sync::LazyLock::new(CcdaConverter::default);

/// Both handlers, sharing one converter.
pub fn operations() -> (CcdaToFhirOperation, FhirToCcdaOperation) {
    let converter = SHARED.clone();
    (
        CcdaToFhirOperation {
            converter: converter.clone(),
        },
        FhirToCcdaOperation { converter },
    )
}

/// The definitions, so the operations are routed and advertised.
pub(crate) fn register_operation_definitions(registry: &mut OperationRegistry) {
    for code in ["ccda-to-fhir", "fhir-to-ccda"] {
        registry.register(OperationDefinition {
            code: code.into(),
            url: format!("http://octofhir.org/OperationDefinition/{code}"),
            kind: OperationKind::Operation,
            system: true,
            type_level: false,
            instance: false,
            resource: vec![],
            parameters: vec![],
            affects_state: false,
        });
    }
}

/// The parameter `name` of a Parameters resource: its `resource`.
fn parameter_resource<'a>(params: &'a Value, name: &str) -> Option<&'a Value> {
    params["parameter"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name)
        .map(|p| &p["resource"])
        .filter(|r| r.is_object())
}

/// The CDA document of a `$ccda-to-fhir` request: a Binary in the `document` (or `input`)
/// parameter, or the Binary that is the body.
fn document_xml(params: &Value) -> Result<String, OperationError> {
    let binary = match params["resourceType"].as_str() {
        Some("Binary") => params,
        Some("Parameters") => parameter_resource(params, "document")
            .or_else(|| parameter_resource(params, "input"))
            .or_else(|| parameter_resource(params, "resource"))
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "expected a Binary in the `document` parameter".into(),
                )
            })?,
        _ => {
            return Err(OperationError::InvalidParameters(
                "expected a Parameters resource with a Binary, or a Binary".into(),
            ));
        }
    };
    if binary["resourceType"] != "Binary" {
        return Err(OperationError::InvalidParameters(
            "the document must be a Binary".into(),
        ));
    }
    let content_type = binary["contentType"].as_str().unwrap_or_default();
    if !(content_type.contains("xml") || content_type.is_empty()) {
        return Err(OperationError::InvalidParameters(format!(
            "the Binary's contentType is `{content_type}`; a CDA document is application/xml"
        )));
    }
    let data = binary["data"]
        .as_str()
        .ok_or_else(|| OperationError::InvalidParameters("the Binary has no data".into()))?;
    if data.len() > MAX_DOCUMENT_BYTES / 3 * 4 + 4 {
        return Err(OperationError::InvalidParameters(format!(
            "the document is larger than {} MiB",
            MAX_DOCUMENT_BYTES / 1024 / 1024
        )));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| {
            OperationError::InvalidParameters(format!("the Binary's data is not base64: {e}"))
        })?;
    String::from_utf8(bytes)
        .map_err(|_| OperationError::InvalidParameters("the document is not UTF-8".into()))
}

/// The document Bundle or Composition of a `$fhir-to-ccda` request.
fn fhir_document(params: &Value) -> Result<&Value, OperationError> {
    match params["resourceType"].as_str() {
        Some("Bundle" | "Composition") => Ok(params),
        Some("Parameters") => parameter_resource(params, "resource")
            .or_else(|| parameter_resource(params, "input"))
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "expected a Bundle or Composition in the `resource` parameter".into(),
                )
            }),
        _ => Err(OperationError::InvalidParameters(
            "expected a document Bundle, a Composition, or Parameters with one".into(),
        )),
    }
}

fn result(value: Value, outcome: Value) -> Value {
    json!({
        "resourceType": "Parameters",
        "parameter": [
            { "name": "return", "resource": value },
            { "name": "outcome", "resource": outcome },
        ]
    })
}

impl CcdaToFhirOperation {
    async fn run(&self, params: &Value) -> Result<Value, OperationError> {
        let xml = document_xml(params)?;
        self.converter
            .with(move |converter| {
                Box::pin(async move {
                    let converted = converter
                        .to_fhir(&xml, &Options::default())
                        .await
                        .map_err(|e| OperationError::InvalidParameters(e.message))?;
                    Ok(result(
                        converted.bundle,
                        converted.report.to_operation_outcome(),
                    ))
                })
            })
            .await
    }
}

impl FhirToCcdaOperation {
    async fn run(&self, params: &Value) -> Result<Value, OperationError> {
        let input = fhir_document(params)?.clone();
        self.converter
            .with(move |converter| {
                Box::pin(async move {
                    let converted = converter
                        .to_cda(&input, &Options::default())
                        .await
                        .map_err(|e| OperationError::InvalidParameters(e.message))?;
                    let binary = json!({
                        "resourceType": "Binary",
                        "contentType": "application/xml",
                        "data": base64::engine::general_purpose::STANDARD.encode(converted.xml.as_bytes()),
                    });
                    Ok(result(binary, converted.report.to_operation_outcome()))
                })
            })
            .await
    }
}

#[async_trait]
impl OperationHandler for CcdaToFhirOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "ccda-to-fhir"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.run(params).await
    }
}

#[async_trait]
impl OperationHandler for FhirToCcdaOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "fhir-to-ccda"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.run(params).await
    }
}

#[derive(serde::Deserialize)]
pub struct CdaRunBody {
    source: String,
    xml: String,
    #[serde(default)]
    debug: bool,
}

/// `POST /api/fml/cda-run`: a map (FML) run over a CDA document, with the trace and the
/// document's positions.
pub async fn cda_run(
    axum::Json(body): axum::Json<CdaRunBody>,
) -> Result<axum::Json<Value>, octofhir_api::ApiError> {
    if body.xml.len() > MAX_DOCUMENT_BYTES {
        return Err(octofhir_api::ApiError::bad_request(format!(
            "the document is larger than {} MiB",
            MAX_DOCUMENT_BYTES / 1024 / 1024
        )));
    }
    let outcome = SHARED
        .with(move |converter| {
            Box::pin(async move {
                let compiled = fml_model::compile(&body.source);
                if compiled.has_errors() {
                    let messages: Vec<String> = compiled
                        .diagnostics
                        .iter()
                        .map(|d| d.message.clone())
                        .collect();
                    return Err(OperationError::InvalidParameters(format!(
                        "the map does not compile: {}",
                        messages.join("; ")
                    )));
                }
                let index = fml_syntax::LineIndex::new(&body.source);
                // The root: the type of the map's source, when it is one of the CDA model's.
                let root = compiled
                    .map
                    .group
                    .first()
                    .and_then(|g| g.input.iter().find(|i| i.mode == "source"))
                    .and_then(|i| i.type_name.as_deref())
                    .and_then(|name| {
                        compiled
                            .map
                            .structure
                            .iter()
                            .find(|s| s.alias.as_deref() == Some(name))
                            .map(|s| s.url.clone())
                    })
                    .filter(|url| converter.model().contains(url));
                let document = converter
                    .read_cda(&body.xml, root.as_deref())
                    .map_err(|e| OperationError::InvalidParameters(e.message))?;
                let spans = |_source: Option<&str>, position: &str| {
                    compiled.locations.span(&body.source, &index, position)
                };
                let run = converter
                    .run_map(&compiled.map, &document.json, body.debug)
                    .await;
                let (result, debug, error) = match run {
                    Ok(transformed) => {
                        let debug = transformed.trace.as_ref().map(|trace| {
                            fml_engine::debug_parameters(
                                &fml_engine::DebugReport {
                                    result: Some(&transformed.target),
                                    trace,
                                    error: None,
                                    warnings: &transformed.warnings,
                                },
                                &spans,
                            )
                        });
                        (Some(transformed.target), debug, None)
                    }
                    Err(error) => {
                        let debug = error.trace.as_ref().map(|trace| {
                            fml_engine::debug_parameters(
                                &fml_engine::DebugReport {
                                    result: None,
                                    trace,
                                    error: Some(&error),
                                    warnings: &[],
                                },
                                &spans,
                            )
                        });
                        (None, debug, Some(error.to_string()))
                    }
                };
                let positions: serde_json::Map<String, Value> = document
                    .positions
                    .iter()
                    .map(|(path, p)| (path.clone(), json!({ "line": p.line, "column": p.column })))
                    .collect();
                let issues: Vec<Value> = document
                    .issues
                    .iter()
                    .map(|issue| {
                        json!({
                            "path": issue.path,
                            "message": issue.message,
                            "line": issue.position.map(|p| p.line),
                            "column": issue.position.map(|p| p.column),
                        })
                    })
                    .collect();
                Ok(json!({
                    "result": result,
                    "debug": debug,
                    "error": error,
                    "input": document.json,
                    "rootType": document.root_type,
                    "positions": positions,
                    "issues": issues,
                }))
            })
        })
        .await
        .map_err(|e| match e {
            OperationError::InvalidParameters(message) => {
                octofhir_api::ApiError::bad_request(message)
            }
            other => octofhir_api::ApiError::internal(other.to_string()),
        })?;
    Ok(axum::Json(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CDA: &str = r#"<ClinicalDocument xmlns="urn:hl7-org:v3"><realmCode code="US"/><typeId root="2.16.840.1.113883.1.3" extension="POCD_HD000040"/><id root="2.16.840.1.113883.19.5" extension="1"/><code code="34133-9" codeSystem="2.16.840.1.113883.6.1"/><title>Summary</title><effectiveTime value="20240102"/><confidentialityCode code="N" codeSystem="2.16.840.1.113883.5.25"/><recordTarget><patientRole><id root="2.16.840.1.113883.19.5" extension="p1"/><patient><name><given>Ann</given><family>Lee</family></name><administrativeGenderCode code="F" codeSystem="2.16.840.1.113883.5.1"/><birthTime value="19700101"/></patient></patientRole></recordTarget><component><structuredBody/></component></ClinicalDocument>"#;

    fn binary(xml: &str) -> Value {
        json!({
            "resourceType": "Binary",
            "contentType": "application/xml",
            "data": base64::engine::general_purpose::STANDARD.encode(xml),
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn both_ways() {
        let (to_fhir, to_cda) = operations();
        let params = json!({ "resourceType": "Parameters", "parameter": [{ "name": "document", "resource": binary(CDA) }] });
        let result = to_fhir.run(&params).await.unwrap();
        let bundle = &result["parameter"][0]["resource"];
        assert_eq!(bundle["type"], "document");
        assert_eq!(
            result["parameter"][1]["resource"]["resourceType"],
            "OperationOutcome"
        );
        let patient = bundle["entry"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["resource"]["resourceType"] == "Patient")
            .unwrap();
        assert_eq!(patient["resource"]["name"][0]["family"], "Lee");

        let back = to_cda.run(bundle).await.unwrap();
        let binary = &back["parameter"][0]["resource"];
        assert_eq!(binary["contentType"], "application/xml");
        let xml = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(binary["data"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(xml.contains("<family>Lee</family>"), "{xml}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bad_input_is_a_parameter_error() {
        let (to_fhir, to_cda) = operations();
        assert!(matches!(
            to_fhir.run(&json!({ "resourceType": "Patient" })).await,
            Err(OperationError::InvalidParameters(_))
        ));
        let not_xml =
            json!({ "resourceType": "Binary", "contentType": "application/pdf", "data": "AAAA" });
        assert!(matches!(
            to_fhir.run(&not_xml).await,
            Err(OperationError::InvalidParameters(_))
        ));
        assert!(matches!(
            to_cda.run(&json!({ "resourceType": "Patient" })).await,
            Err(OperationError::InvalidParameters(_))
        ));
    }
}
