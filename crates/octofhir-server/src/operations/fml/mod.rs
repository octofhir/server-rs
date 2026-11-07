//! FHIR Mapping Language operations, on the [fml-rs](https://github.com/octofhir/fml-rs) engine:
//! `StructureMap/$transform`, the StructureMap-based `QuestionnaireResponse/$extract`, and
//! `$convert` between FHIR versions with the cross-version maps.

pub mod api;
mod check;
mod convert;
mod extract;
mod library;
pub(crate) mod resolve;
mod transform;

use std::sync::Arc;

pub(crate) use check::structure_map_issues;
pub use convert::ConvertOperation;
pub(crate) use convert::outcome_header;
pub use extract::ExtractOperation;
pub use transform::TransformOperation;

use crate::config::FmlConfig;

/// The FML operations, sharing one engine runtime.
pub fn operations(config: FmlConfig) -> (TransformOperation, ExtractOperation, ConvertOperation) {
    let runner = Arc::new(transform::Runner::new(config));
    (
        TransformOperation::new(runner.clone()),
        ExtractOperation::new(runner.clone()),
        ConvertOperation::new(runner),
    )
}

/// The definitions of the FML operations, so they are routed and advertised whether or not a
/// package that defines them is installed (SDC usually is not).
pub(crate) fn register_operation_definitions(
    registry: &mut crate::operations::OperationRegistry,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    for (code, url, resource) in [
        (
            "transform",
            "http://hl7.org/fhir/OperationDefinition/StructureMap-transform",
            Some("StructureMap"),
        ),
        (
            "extract",
            "http://hl7.org/fhir/uv/sdc/OperationDefinition/QuestionnaireResponse-extract",
            Some("QuestionnaireResponse"),
        ),
        (
            "convert",
            "http://hl7.org/fhir/OperationDefinition/Resource-convert",
            None,
        ),
    ] {
        if registry.get_by_url(url).is_some() {
            continue;
        }
        registry.register(crate::operations::OperationDefinition {
            code: code.into(),
            url: url.into(),
            kind: crate::operations::OperationKind::Operation,
            system: resource.is_none(),
            type_level: resource.is_some(),
            instance: resource.is_some(),
            resource: resource.into_iter().map(str::to_owned).collect(),
            parameters: vec![],
            affects_state: false,
        });
    }
}
