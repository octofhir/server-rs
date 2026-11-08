//! Patient-scoped DICOMweb metadata import through the normal FHIR write pipeline.
//!
//! # Contents
//! `$dicom-import` definition, strict Parameters decoding and atomic Endpoint/study writes.
//!
//! # Invariants
//! The caller must read the patient and imported resources and be allowed to write
//! each changed resource. A PACS credential confers no FHIR privileges. Import never
//! stores images. Conditional versions prevent overwriting concurrent changes.

use super::{
    OperationContext, OperationDefinition, OperationError, OperationHandler, OperationKind,
    OperationParameter, OperationRegistry, ParameterUse, handler::OperationLevel,
};
use crate::{
    dicom::{DicomError, mapping, validate_uid},
    server::AppState,
};
use async_trait::async_trait;
use octofhir_api::ApiError;
use serde_json::{Value, json};

pub struct DicomImportOperation;

pub(crate) fn register_definition(registry: &mut OperationRegistry, enabled: bool) {
    if !enabled {
        return;
    }
    let input = |name: &str| OperationParameter {
        name: name.into(),
        use_: ParameterUse::In,
        min: 1,
        max: "1".into(),
        param_type: Some("string".into()),
        search_type: None,
        target_profile: vec![],
        parts: vec![],
    };
    registry.replace(OperationDefinition {
        code: "dicom-import".into(),
        url: "https://octofhir.org/OperationDefinition/dicom-import".into(),
        kind: OperationKind::Operation,
        system: false,
        type_level: false,
        instance: true,
        resource: vec!["Patient".into()],
        affects_state: true,
        parameters: vec![
            input("connection"),
            input("study"),
            OperationParameter {
                name: "imagingStudy".into(),
                use_: ParameterUse::Out,
                min: 1,
                max: "1".into(),
                param_type: Some("Reference".into()),
                search_type: None,
                target_profile: vec!["http://hl7.org/fhir/StructureDefinition/ImagingStudy".into()],
                parts: vec![],
            },
            OperationParameter {
                name: "endpoint".into(),
                use_: ParameterUse::Out,
                min: 1,
                max: "1".into(),
                param_type: Some("Reference".into()),
                search_type: None,
                target_profile: vec!["http://hl7.org/fhir/StructureDefinition/Endpoint".into()],
                parts: vec![],
            },
            OperationParameter {
                name: "changed".into(),
                use_: ParameterUse::Out,
                min: 1,
                max: "1".into(),
                param_type: Some("boolean".into()),
                search_type: None,
                target_profile: vec![],
                parts: vec![],
            },
        ],
    });
}

fn parameters(params: &Value) -> Result<(&str, &str), OperationError> {
    let invalid = || {
        OperationError::InvalidParameters(
            "Expected exactly one connection and study valueString in Parameters".into(),
        )
    };
    if params["resourceType"] != "Parameters" {
        return Err(invalid());
    }
    let entries = params["parameter"]
        .as_array()
        .filter(|items| items.len() == 2)
        .ok_or_else(invalid)?;
    let mut connection = None;
    let mut study = None;
    for parameter in entries {
        let object = parameter.as_object().ok_or_else(invalid)?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "valueString"))
        {
            return Err(invalid());
        }
        let value = parameter["valueString"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)?;
        let slot = match parameter["name"].as_str() {
            Some("connection") => &mut connection,
            Some("study") => &mut study,
            _ => return Err(invalid()),
        };
        if slot.replace(value).is_some() {
            return Err(invalid());
        }
    }
    let study = study.ok_or_else(invalid)?;
    validate_uid(study).map_err(|_| {
        OperationError::InvalidParameters(
            "study requires a DICOM UID without urn:oid: prefix".into(),
        )
    })?;
    Ok((connection.ok_or_else(invalid)?, study))
}

pub(crate) fn dicom_error(error: DicomError) -> OperationError {
    match error {
        DicomError::Partial => {
            ApiError::conflict("DICOM study metadata is incomplete; retry later").into()
        }
        DicomError::Metadata(_) | DicomError::Limit => {
            ApiError::unprocessable_entity(error.to_string(), None).into()
        }
        DicomError::Status(404) => {
            OperationError::NotFound("Study metadata is unavailable at the configured PACS".into())
        }
        _ => OperationError::Internal(error.to_string()),
    }
}

/// Read under the caller, preserve fields outside the projection and skip identical writes.
pub(crate) async fn entry(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    resource: Value,
) -> Result<Option<Value>, OperationError> {
    let kind = resource["resourceType"].as_str().unwrap();
    let id = resource["id"].as_str().unwrap();
    let previous = context.read_primary(state, kind, id).await?;
    let (resource, version) = if let Some(previous) = previous {
        let merged =
            mapping::merge(&previous.resource, &resource, connection).map_err(dicom_error)?;
        if previous.resource == merged {
            return Ok(None);
        }
        (merged, Some(previous.version_id))
    } else {
        (resource, None)
    };
    let mut request = json!({"method":"PUT","url":format!("{}/{}",resource["resourceType"].as_str().unwrap(),resource["id"].as_str().unwrap())});
    if let Some(version) = version {
        request["ifMatch"] = json!(format!("W/\"{version}\""));
    }
    Ok(Some(json!({"request":request,"resource":resource})))
}

#[async_trait]
impl OperationHandler for DicomImportOperation {
    fn code(&self) -> &str {
        "dicom-import"
    }

    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance && resource_type == Some("Patient")
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        patient_id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Patient" || context.is_get() {
            return Err(OperationError::InvalidParameters(
                "Use POST Patient/{id}/$dicom-import".into(),
            ));
        }
        let (connection, study_uid) = parameters(params)?;
        let result = crate::dicom::import::import(
            state,
            context,
            connection,
            study_uid,
            Some(patient_id),
            None,
            None,
        )
        .await?;
        Ok(json!({"resourceType":"Parameters","parameter":[
            {"name":"imagingStudy","valueReference":{"reference":format!("ImagingStudy/{}",result.study_id)}},
            {"name":"endpoint","valueReference":{"reference":format!("Endpoint/{}",result.endpoint_id)}},
            {"name":"changed","valueBoolean":result.changed}
        ]}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_reject_duplicates_unknowns_and_resource_injection() {
        let good = json!({"resourceType":"Parameters","parameter":[
            {"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"}
        ]});
        assert_eq!(parameters(&good).unwrap(), ("pacs", "1.2.3"));
        for bad in [
            json!({"resourceType":"Patient"}),
            json!({"resourceType":"Parameters","parameter":[{"name":"study","valueString":"1.2.3"},{"name":"study","valueString":"1.2.4"}]}),
            json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs","resource":{}},{"name":"study","valueString":"1.2.3"}]}),
        ] {
            assert!(parameters(&bad).is_err());
        }
    }

    #[test]
    fn definition_is_only_advertised_when_configured() {
        let mut registry = OperationRegistry::new();
        register_definition(&mut registry, false);
        assert!(
            registry
                .get_instance_operation("Patient", "dicom-import")
                .is_none()
        );
        register_definition(&mut registry, true);
        assert!(
            registry
                .get_instance_operation("Patient", "dicom-import")
                .unwrap()
                .affects_state
        );
        assert!(registry.get_system_operation("dicom-import").is_none());
    }
}
