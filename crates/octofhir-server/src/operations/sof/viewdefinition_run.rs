//! `$viewdefinition-run` (SQL on FHIR): execute a ViewDefinition and return
//! its rows in the requested format.
//!
//! Defined on system, type and instance level by the SQL on FHIR IG
//! (`OperationDefinition/$viewdefinition-run`). The single output parameter
//! `return` is a Binary, so the response is the rows themselves in the
//! format's media type (ndjson by default), not a Parameters envelope.
//!
//! Parameters: `viewReference` | `viewResource` (exactly one at system and
//! type level; none at instance level), `resource` (inline data instead of
//! the server's), `patient`, `group`, `_since` (data filters), `_limit`,
//! `_format` (`ndjson`, `json`, `csv`, `fhir`), `header` (csv). `source` (an
//! external data source) is not supported.

use async_trait::async_trait;
use octofhir_sof::ViewDefinition;
use serde_json::Value;

use super::engine;
use super::output::{OutputColumn, OutputFormat, render};
use super::scope::ViewScope;
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// The `$viewdefinition-run` handler.
pub struct ViewDefinitionRunSpecOperation {
    enabled: bool,
}

impl ViewDefinitionRunSpecOperation {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn check_enabled(&self) -> Result<(), OperationError> {
        if self.enabled {
            Ok(())
        } else {
            Err(OperationError::NotSupported(
                "SQL on FHIR is not enabled. Please set sql_on_fhir.enabled = true in configuration."
                    .into(),
            ))
        }
    }

    /// The view named by `viewReference` or supplied as `viewResource`
    /// (also accepted under its pre-ballot name `viewDefinition`).
    async fn view_from_parameters(
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<ViewDefinition, OperationError> {
        let mut references = Vec::new();
        let mut resources = Vec::new();
        for param in parameters(params) {
            match param["name"].as_str() {
                Some("viewReference") => references.push(param),
                Some("viewResource" | "viewDefinition") => resources.push(param),
                _ => {}
            }
        }
        match (references.as_slice(), resources.as_slice()) {
            ([reference], []) => {
                let reference = reference["valueReference"]["reference"]
                    .as_str()
                    .or_else(|| reference["valueString"].as_str())
                    .ok_or_else(|| {
                        OperationError::InvalidParameters(
                            "viewReference requires a ViewDefinition reference".into(),
                        )
                    })?;
                let id = reference
                    .strip_prefix("ViewDefinition/")
                    .unwrap_or(reference);
                octofhir_core::validate_id(id).map_err(|_| {
                    OperationError::InvalidParameters(
                        "viewReference requires a local ViewDefinition/id".into(),
                    )
                })?;
                Self::stored_view(state, context, id).await
            }
            ([], [resource]) => ViewDefinition::from_json(&resource["resource"]).map_err(|error| {
                OperationError::InvalidParameters(format!("Invalid ViewDefinition: {error}"))
            }),
            _ => Err(OperationError::InvalidParameters(
                "Exactly one of viewReference and viewResource is required".into(),
            )),
        }
    }

    async fn stored_view(
        state: &AppState,
        context: &OperationContext,
        id: &str,
    ) -> Result<ViewDefinition, OperationError> {
        let stored = context
            .read(state, "ViewDefinition", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("ViewDefinition/{id} not found")))?;
        ViewDefinition::from_json(&stored.resource).map_err(|error| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {error}"))
        })
    }
}

/// Run `view` with the operation's data parameters (`resource`, `patient`,
/// `group`, `_since`, `_limit`) and render it in `_format`. Shared by
/// `$viewdefinition-run` and `$sql-run` with a ViewDefinition subject.
pub(super) async fn run_view(
    state: &AppState,
    context: &OperationContext,
    view: &ViewDefinition,
    params: &Value,
) -> Result<Value, OperationError> {
    if parameters(params).any(|param| param["name"] == "source") {
        return Err(OperationError::NotSupported(
            "External data sources (source) are not supported; the view reads this server's data"
                .into(),
        ));
    }
    let format = output_format(params)?;
    let limit = limit(params)?;
    let inline = inline_resources(params)?;
    let scope = ViewScope::from_parameters(params)?;
    if inline.is_some() && !scope.is_empty() {
        return Err(OperationError::InvalidParameters(
            "patient, group and _since filter the server's data, not inline resources".into(),
        ));
    }
    let search = scope.search_params(state, context, &view.resource).await?;

    let (columns, rows) = engine::execute(state, context, view, inline, &search, limit).await?;
    let columns: Vec<OutputColumn> = columns.iter().map(OutputColumn::from_view).collect();
    render(
        format,
        &columns,
        &rows,
        state.config.search.include_limits.max_bytes,
    )
}

#[async_trait]
impl OperationHandler for ViewDefinitionRunSpecOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        self.enabled && (level == OperationLevel::System || resource_type == Some("ViewDefinition"))
    }

    fn code(&self) -> &str {
        "viewdefinition-run"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;
        let view = Self::view_from_parameters(state, context, params).await?;
        run_view(state, context, &view, params).await
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;
        only_view_definition(resource_type)?;
        let view = Self::view_from_parameters(state, context, params).await?;
        run_view(state, context, &view, params).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;
        only_view_definition(resource_type)?;
        if parameters(params).any(|param| {
            matches!(
                param["name"].as_str(),
                Some("viewReference" | "viewResource")
            )
        }) {
            return Err(OperationError::InvalidParameters(
                "The instance names the ViewDefinition; viewReference and viewResource do not apply"
                    .into(),
            ));
        }
        let view = Self::stored_view(state, context, id).await?;
        run_view(state, context, &view, params).await
    }
}

fn only_view_definition(resource_type: &str) -> Result<(), OperationError> {
    if resource_type == "ViewDefinition" {
        Ok(())
    } else {
        Err(OperationError::NotSupported(format!(
            "$viewdefinition-run runs a ViewDefinition, not {resource_type}"
        )))
    }
}

pub(super) fn parameters(params: &Value) -> impl Iterator<Item = &Value> {
    params["parameter"].as_array().into_iter().flatten()
}

/// The value of a parameter that occurs at most once, as text.
pub(super) fn single_text(
    params: &Value,
    name: &str,
    fields: &[&str],
) -> Result<Option<String>, OperationError> {
    let mut found = parameters(params).filter(|param| param["name"] == name);
    let Some(param) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() {
        return Err(OperationError::InvalidParameters(format!(
            "{name} must occur at most once"
        )));
    }
    fields
        .iter()
        .find_map(|field| param[*field].as_str())
        .map(|text| Some(text.to_owned()))
        .ok_or_else(|| OperationError::InvalidParameters(format!("{name} has no value")))
}

/// `_limit`: a non-negative integer (`valueInteger`, or a GET string).
pub(super) fn limit(params: &Value) -> Result<Option<usize>, OperationError> {
    let invalid =
        || OperationError::InvalidParameters("_limit must be a non-negative integer".into());
    let mut found = parameters(params).filter(|param| param["name"] == "_limit");
    let Some(param) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() {
        return Err(OperationError::InvalidParameters(
            "_limit must occur at most once".into(),
        ));
    }
    let value = match (&param["valueInteger"], param["valueString"].as_str()) {
        (Value::Number(number), _) => number.as_u64().ok_or_else(invalid)?,
        (_, Some(text)) => text.parse::<u64>().map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    usize::try_from(value).map(Some).map_err(|_| invalid())
}

/// `header` (csv only): `valueBoolean`, or `true`/`false` from GET; default true.
pub(super) fn header(params: &Value) -> Result<bool, OperationError> {
    let Some(param) = parameters(params).find(|param| param["name"] == "header") else {
        return Ok(true);
    };
    match (&param["valueBoolean"], param["valueString"].as_str()) {
        (Value::Bool(value), _) => Ok(*value),
        (_, Some("true")) => Ok(true),
        (_, Some("false")) => Ok(false),
        _ => Err(OperationError::InvalidParameters(
            "header must be a boolean".into(),
        )),
    }
}

/// `resource` (0..*): FHIR resources to transform instead of the server's
/// data; a Bundle contributes its entries.
pub(super) fn inline_resources(params: &Value) -> Result<Option<Vec<Value>>, OperationError> {
    let mut resources = Vec::new();
    for param in parameters(params).filter(|param| param["name"] == "resource") {
        let resource = param
            .get("resource")
            .filter(|resource| resource["resourceType"].is_string())
            .ok_or_else(|| {
                OperationError::InvalidParameters("resource requires a FHIR resource".into())
            })?;
        if resource["resourceType"] == "Bundle" {
            resources.extend(
                resource["entry"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.get("resource").cloned()),
            );
        } else {
            resources.push(resource.clone());
        }
    }
    Ok((!resources.is_empty()).then_some(resources))
}

/// `_format` (default ndjson) with `header` for csv.
pub(super) fn output_format(params: &Value) -> Result<OutputFormat, OperationError> {
    let format = single_text(params, "_format", &["valueCode", "valueString"])?;
    OutputFormat::parse(format.as_deref(), header(params)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limit_reads_integers_and_get_strings_and_rejects_the_rest() {
        let p = |v: Value| json!({"parameter":[v]});
        assert_eq!(
            limit(&p(json!({"name":"_limit","valueInteger":5}))).unwrap(),
            Some(5)
        );
        assert_eq!(
            limit(&p(json!({"name":"_limit","valueString":"7"}))).unwrap(),
            Some(7)
        );
        assert!(limit(&p(json!({"name":"_limit","valueInteger":-1}))).is_err());
        assert!(limit(&p(json!({"name":"_limit","valueString":"x"}))).is_err());
        assert_eq!(limit(&json!({"parameter":[]})).unwrap(), None);
    }

    #[test]
    fn a_bundle_resource_contributes_its_entries() {
        let params = json!({"parameter":[{"name":"resource","resource":{
        "resourceType":"Bundle","type":"collection","entry":[
            {"resource":{"resourceType":"Patient","id":"a"}},
            {"resource":{"resourceType":"Patient","id":"b"}}
        ]}}]});
        assert_eq!(inline_resources(&params).unwrap().unwrap().len(), 2);
    }
}
