//! `Measure/[id]/$data-requirements` and `Library/[id]/$data-requirements`: the data, the
//! terminology and the libraries a Measure or Library needs, as a `module-definition`
//! Library.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::cql::{map_cql_error, scoped_service};
use super::evaluate_measure::{measure_library, text_parameter};
use super::handler::OperationLevel;
use super::{
    OperationContext, OperationDefinition, OperationError, OperationHandler, OperationKind,
};
use crate::server::AppState;

const CODE: &str = "data-requirements";

/// The definitions of the operation, one per resource it is defined on.
pub fn definitions() -> [OperationDefinition; 2] {
    ["Measure", "Library"].map(|resource| OperationDefinition {
        code: CODE.to_owned(),
        url: format!("http://hl7.org/fhir/OperationDefinition/{resource}-data-requirements"),
        kind: OperationKind::Operation,
        system: false,
        type_level: false,
        instance: true,
        resource: vec![resource.to_owned()],
        parameters: vec![],
        affects_state: false,
    })
}

/// Adds the Measure's own library to the dependencies of the result.
fn depends_on(result: &mut Value, library: &str) {
    let artifact = json!({"type": "depends-on", "resource": library});
    match result["relatedArtifact"].as_array_mut() {
        Some(artifacts) => artifacts.insert(0, artifact),
        None => result["relatedArtifact"] = json!([artifact]),
    }
}

pub struct DataRequirementsOperation;

#[async_trait]
impl OperationHandler for DataRequirementsOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance && matches!(resource_type, Some("Measure" | "Library"))
    }

    fn code(&self) -> &str {
        CODE
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".into()))?;
        let service = scoped_service(state, context, service);
        let stored = context
            .read(state, resource_type, id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("{resource_type}/{id} not found")))?;
        let resource = &stored.resource;

        if resource_type == "Library" {
            let url = resource["url"].as_str().ok_or_else(|| {
                OperationError::InvalidParameters("The Library has no url".into())
            })?;
            let source = service
                .library_source(url, resource["version"].as_str())
                .await
                .map_err(map_cql_error)?;
            return service
                .data_requirements(&source)
                .await
                .map_err(map_cql_error);
        }

        for name in ["periodStart", "periodEnd"] {
            if text_parameter(params, name).is_none() {
                return Err(OperationError::InvalidParameters(format!(
                    "{name} is required"
                )));
            }
        }
        let (library, source) = measure_library(&service, resource).await?;
        let mut result = service
            .data_requirements(&source)
            .await
            .map_err(map_cql_error)?;
        depends_on(&mut result, library);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_resource_has_its_own_definition() {
        let [measure, library] = definitions();
        assert_eq!(
            measure.url,
            "http://hl7.org/fhir/OperationDefinition/Measure-data-requirements"
        );
        assert_eq!(library.resource, ["Library"]);
        assert!(measure.instance && !measure.type_level && !measure.system);
    }

    #[test]
    fn the_measure_library_comes_first_among_dependencies() {
        let mut result = json!({"relatedArtifact":[{"type":"depends-on","resource":"b"}]});
        depends_on(&mut result, "a");
        assert_eq!(result["relatedArtifact"][0]["resource"], "a");
        let mut empty = json!({});
        depends_on(&mut empty, "a");
        assert_eq!(empty["relatedArtifact"][0]["resource"], "a");
    }
}
