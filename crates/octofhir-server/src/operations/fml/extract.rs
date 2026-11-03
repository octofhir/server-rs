//! `QuestionnaireResponse/$extract` and `QuestionnaireResponse/[id]/$extract` (SDC), the
//! StructureMap-based form: the Questionnaire names its map with the
//! `sdc-questionnaire-targetStructureMap` extension, and the map runs over the response.
//!
//! Inputs: the instance or `questionnaire-response`; optionally `questionnaire` (otherwise the
//! one the response names, found as the caller); `debug` as for `$transform`. The result is what
//! the map produces, usually a transaction Bundle. Definition-based and observation-based
//! extraction are not implemented: a Questionnaire without a target map is a 422.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_api::ApiError;
use serde_json::Value;

use super::resolve;
use super::transform::{Ran, Runner, Sources};
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

const TARGET_STRUCTURE_MAP: &str =
    "http://hl7.org/fhir/uv/sdc/StructureDefinition/sdc-questionnaire-targetStructureMap";

/// Handler for `$extract` on QuestionnaireResponse.
pub struct ExtractOperation {
    runner: Arc<Runner>,
}

impl ExtractOperation {
    pub(crate) fn new(runner: Arc<Runner>) -> Self {
        Self { runner }
    }

    async fn extract(
        &self,
        state: &AppState,
        context: &OperationContext,
        response: Value,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let debug = parameters(params).any(|p| {
            p["name"] == "debug"
                && (p["valueBoolean"].as_bool() == Some(true) || p["valueString"] == "true")
        });
        if debug {
            self.runner.allow_debug(context)?;
        }
        let questionnaire = match resource_parameter(params, "questionnaire") {
            Some(questionnaire) => questionnaire,
            None => {
                let canonical = response["questionnaire"].as_str().ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "The QuestionnaireResponse names no questionnaire; give `questionnaire`"
                            .into(),
                    )
                })?;
                resolve::canonical_resource(state, context, "Questionnaire", canonical)
                    .await?
                    .ok_or_else(|| OperationError::NotFound(format!("Questionnaire {canonical}")))?
            }
        };
        let target = questionnaire["extension"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|extension| extension["url"] == TARGET_STRUCTURE_MAP)
            .and_then(|extension| extension["valueCanonical"].as_str())
            .ok_or_else(|| {
                OperationError::Api(ApiError::unprocessable_entity(
                    "The Questionnaire has no sdc-questionnaire-targetStructureMap; only \
                     StructureMap-based extraction is supported",
                    None,
                ))
            })?;
        let map = match target.strip_prefix('#') {
            Some(id) => {
                let contained = questionnaire["contained"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|resource| {
                        resource["resourceType"] == "StructureMap" && resource["id"] == id
                    })
                    .cloned()
                    .ok_or_else(|| {
                        OperationError::NotFound(format!("contained StructureMap #{id}"))
                    })?;
                resolve::map_from_json(contained, &format!("StructureMap #{id}"))?
            }
            None => {
                let library = self.runner.library().await?;
                resolve::structure_map(state, context, &library, target)
                    .await?
                    .ok_or_else(|| OperationError::NotFound(format!("StructureMap {target}")))?
                    .as_ref()
                    .clone()
            }
        };
        self.runner
            .run(
                state,
                context,
                vec![map],
                &Sources::default(),
                response,
                Default::default(),
                debug,
            )
            .await
            .map(Ran::into_response)
    }
}

fn parameters(params: &Value) -> impl Iterator<Item = &Value> {
    params["parameter"].as_array().into_iter().flatten()
}

fn resource_parameter(params: &Value, name: &str) -> Option<Value> {
    parameters(params)
        .find(|p| p["name"] == name)
        .and_then(|p| p.get("resource").cloned())
}

#[async_trait]
impl OperationHandler for ExtractOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        self.runner.config.enabled
            && matches!(level, OperationLevel::Type | OperationLevel::Instance)
            && resource_type.is_none_or(|t| t == "QuestionnaireResponse")
    }

    fn code(&self) -> &str {
        "extract"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        check_type(&self.runner, resource_type)?;
        let response = resource_parameter(params, "questionnaire-response")
            .or_else(|| resource_parameter(params, "resource"))
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "Missing parameter `questionnaire-response`".into(),
                )
            })?;
        self.extract(state, context, response, params).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        check_type(&self.runner, resource_type)?;
        let response = context
            .read(state, "QuestionnaireResponse", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("QuestionnaireResponse/{id}")))?
            .resource;
        self.extract(state, context, response, params).await
    }
}

fn check_type(runner: &Runner, resource_type: &str) -> Result<(), OperationError> {
    if !runner.config.enabled {
        return Err(OperationError::NotSupported(
            "$extract is disabled on this server".into(),
        ));
    }
    if resource_type != "QuestionnaireResponse" {
        return Err(OperationError::NotSupported(format!(
            "$extract is defined on QuestionnaireResponse, not {resource_type}"
        )));
    }
    Ok(())
}
