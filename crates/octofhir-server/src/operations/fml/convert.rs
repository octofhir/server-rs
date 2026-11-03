//! `$convert` between FHIR versions, with the HL7 cross-version StructureMaps.
//!
//! The FHIR operation (`Resource-convert`) takes the resource in `input` (or as the body) and
//! returns it in the representation the client asks for. Here the representations are FHIR
//! versions: the input is in the version of the request's `Content-Type` (`fhirVersion=4.0`,
//! the server's own when absent), the output in the version of `Accept`. The map is the
//! cross-version map for the type, `http://hl7.org/fhir/uv/xver/StructureMap/{Type}4to5`,
//! found like any other map (the server's StructureMaps, its packages, `fml.map_folders`).
//!
//! The response is the converted resource with `Content-Type: application/fhir+json;
//! fhirVersion=…`. Values the maps could not carry over (a code a ConceptMap does not know) are
//! listed as warnings in an OperationOutcome in the `X-Conversion-Outcome` header.

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use octofhir_api::{ApiError, OperationOutcomeIssue};
use serde_json::{Value, json};

use super::resolve;
use super::transform::{Ran, Runner, Sources};
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Marks a converted resource for the router: the Binary's content is the response body, and
/// this key carries the conversion warnings for the `X-Conversion-Outcome` header.
pub(crate) const CONVERSION_OUTCOME_KEY: &str = "__fmlConversionOutcome";

/// Handler for `$convert`.
pub struct ConvertOperation {
    runner: Arc<Runner>,
}

/// A FHIR release as the cross-version maps name it in their URLs, and its `fhirVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Version {
    /// `4`, `4B`, `5`, …
    map_code: &'static str,
    /// `4.0`, `4.3`, `5.0`, …
    fhir_version: &'static str,
}

const VERSIONS: &[Version] = &[
    Version {
        map_code: "2",
        fhir_version: "1.0",
    },
    Version {
        map_code: "3",
        fhir_version: "3.0",
    },
    Version {
        map_code: "4",
        fhir_version: "4.0",
    },
    Version {
        map_code: "4B",
        fhir_version: "4.3",
    },
    Version {
        map_code: "5",
        fhir_version: "5.0",
    },
    Version {
        map_code: "6",
        fhir_version: "6.0",
    },
];

impl Version {
    /// From a `fhirVersion` media type parameter: `4.0`, `4.0.1`, `5.0.0-ballot`.
    fn from_fhir_version(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let major_minor = format!("{}.{}", parts.next()?, parts.next()?);
        VERSIONS
            .iter()
            .find(|v| v.fhir_version == major_minor)
            .copied()
    }

    /// The server's release (`R4`, `R4B`, `R5`, `R6`).
    fn from_release(release: &str) -> Option<Self> {
        let code = match release {
            "R4" => "4",
            "R4B" => "4B",
            "R5" => "5",
            "R6" => "6",
            _ => return None,
        };
        VERSIONS.iter().find(|v| v.map_code == code).copied()
    }
}

/// The `fhirVersion` parameter of a FHIR JSON media type, if the header names one.
fn media_type_version(header: Option<&str>) -> Result<Option<Version>, OperationError> {
    let Some(header) = header else {
        return Ok(None);
    };
    // The first media range that is FHIR JSON decides.
    for range in header.split(',') {
        let mut parts = range.split(';').map(str::trim);
        let media = parts.next().unwrap_or_default();
        if !media.eq_ignore_ascii_case("application/fhir+json")
            && !media.eq_ignore_ascii_case("application/json")
        {
            continue;
        }
        for parameter in parts {
            if let Some((name, value)) = parameter.split_once('=')
                && name.trim().eq_ignore_ascii_case("fhirVersion")
            {
                let value = value.trim().trim_matches('"');
                return Version::from_fhir_version(value).map(Some).ok_or_else(|| {
                    OperationError::Api(ApiError::issues(
                        axum::http::StatusCode::NOT_ACCEPTABLE,
                        vec![OperationOutcomeIssue::error(
                            "not-supported",
                            format!("fhirVersion {value} is not one this server converts"),
                            None,
                        )],
                    ))
                });
            }
        }
        return Ok(None);
    }
    Ok(None)
}

impl ConvertOperation {
    pub(crate) fn new(runner: Arc<Runner>) -> Self {
        Self { runner }
    }

    async fn convert(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let input = params["parameter"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["name"] == "input" || p["name"] == "resource")
            .and_then(|p| p.get("resource").cloned())
            .ok_or_else(|| OperationError::InvalidParameters("Missing parameter `input`".into()))?;
        let resource_type = input["resourceType"]
            .as_str()
            .ok_or_else(|| OperationError::InvalidParameters("`input` has no resourceType".into()))?
            .to_owned();
        let server = Version::from_release(&state.fhir_version).ok_or_else(|| {
            OperationError::NotSupported(format!(
                "$convert does not support FHIR {}",
                state.fhir_version
            ))
        })?;
        let from = media_type_version(context.content_type())?.unwrap_or(server);
        let to = media_type_version(context.accept())?.unwrap_or(server);
        if from == to {
            return Ok(output(input, to, &[]));
        }
        let url = format!(
            "http://hl7.org/fhir/uv/xver/StructureMap/{resource_type}{}to{}",
            from.map_code, to.map_code
        );
        let library = self.runner.library().await?;
        let map = resolve::structure_map(state, context, &library, &url)
            .await?
            .ok_or_else(|| {
                OperationError::Api(ApiError::issues(
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    vec![OperationOutcomeIssue::error(
                        "not-supported",
                        format!(
                            "No map converts {resource_type} from FHIR {} to {} ({url}); add \
                             the cross-version maps to the server or to fml.map_folders",
                            from.fhir_version, to.fhir_version
                        ),
                        None,
                    )],
                ))
            })?;
        match self
            .runner
            .run(
                state,
                context,
                vec![map.as_ref().clone()],
                &Sources::default(),
                input,
                Default::default(),
                false,
            )
            .await?
        {
            Ran::Done {
                target, warnings, ..
            } => Ok(output(target, to, &warnings)),
            Ran::Debug(parameters) => Ok(parameters),
        }
    }
}

/// The conversion warnings of a [`output`] Binary as an `X-Conversion-Outcome` header value:
/// compact JSON with everything outside ASCII escaped, at most 8 KiB (later issues dropped).
pub(crate) fn outcome_header(binary: &Value) -> Option<String> {
    let outcome = binary.get(CONVERSION_OUTCOME_KEY)?;
    let mut outcome = outcome.clone();
    loop {
        let text = ascii_json(&outcome);
        if text.len() <= 8 * 1024 {
            return Some(text);
        }
        let issues = outcome["issue"].as_array_mut()?;
        if issues.len() <= 1 {
            return None;
        }
        issues.pop();
    }
}

fn ascii_json(value: &Value) -> String {
    let mut out = String::new();
    for c in value.to_string().chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// The converted resource, for the router to write as the body in its version's media type.
fn output(resource: Value, version: Version, warnings: &[String]) -> Value {
    let mut binary = json!({
        "resourceType": "Binary",
        "contentType": format!("application/fhir+json; fhirVersion={}", version.fhir_version),
        "data": base64::engine::general_purpose::STANDARD.encode(resource.to_string()),
    });
    if !warnings.is_empty() {
        let issues: Vec<Value> = warnings
            .iter()
            .map(|warning| {
                json!({ "severity": "warning", "code": "informational", "diagnostics": warning })
            })
            .collect();
        binary[CONVERSION_OUTCOME_KEY] =
            json!({ "resourceType": "OperationOutcome", "issue": issues });
    }
    binary
}

#[async_trait]
impl OperationHandler for ConvertOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.runner.config.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "convert"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if !self.runner.config.enabled {
            return Err(OperationError::NotSupported(
                "$convert is disabled on this server".into(),
            ));
        }
        self.convert(state, context, params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_from_media_types() {
        let version = |header: &str| {
            media_type_version(Some(header))
                .unwrap()
                .map(|v| v.map_code)
        };
        assert_eq!(version("application/fhir+json; fhirVersion=5.0"), Some("5"));
        assert_eq!(
            version("application/fhir+json;fhirVersion=4.0.1"),
            Some("4")
        );
        assert_eq!(
            version("application/fhir+json; fhirVersion=\"4.3\""),
            Some("4B")
        );
        assert_eq!(version("application/fhir+json"), None);
        assert_eq!(
            version("text/html, application/fhir+json; fhirVersion=3.0"),
            Some("3")
        );
        assert!(media_type_version(Some("application/fhir+json; fhirVersion=9.9")).is_err());
    }
}
