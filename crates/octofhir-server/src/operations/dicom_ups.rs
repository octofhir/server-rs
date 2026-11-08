//! Task-scoped built-in UPS-RS Requestor/Watcher operation.
//!
//! # Invariants
//! Only enabled sources advertise this operation. The implementation admits a
//! concrete readable/update-authorized Task and never accepts a raw PACS payload.
use super::{
    OperationContext, OperationDefinition, OperationError, OperationHandler, OperationKind,
    OperationParameter, OperationRegistry, ParameterUse, handler::OperationLevel,
};
use crate::server::AppState;
use async_trait::async_trait;
use serde_json::Value;
pub struct DicomUpsOperation;
pub(crate) fn register_definition(registry: &mut OperationRegistry, enabled: bool) {
    if !enabled {
        return;
    }
    registry.replace(OperationDefinition {
        code: "dicom-ups".into(),
        url: "https://octofhir.org/OperationDefinition/dicom-ups".into(),
        kind: OperationKind::Operation,
        system: false,
        type_level: false,
        instance: true,
        resource: vec!["Task".into()],
        affects_state: true,
        parameters: ["connection", "action"]
            .into_iter()
            .map(|name| OperationParameter {
                name: name.into(),
                use_: ParameterUse::In,
                min: 1,
                max: "1".into(),
                param_type: Some("string".into()),
                search_type: None,
                target_profile: vec![],
                parts: vec![],
            })
            .collect(),
    });
}
#[async_trait]
impl OperationHandler for DicomUpsOperation {
    fn code(&self) -> &str {
        "dicom-ups"
    }
    fn supports_level(&self, level: OperationLevel, kind: Option<&str>) -> bool {
        level == OperationLevel::Instance && kind == Some("Task")
    }
    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        _: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        crate::dicom::ups::operate(state, context, id, params).await
    }
}
