//! Explicit contract for measure evaluation, pending a population evaluator.
//! CQL expression evaluation alone does not implement FHIR measure scoring.

use async_trait::async_trait;
use serde_json::Value;

use super::{OperationError, OperationHandler};
use crate::server::AppState;

pub struct EvaluateMeasureOperation;

impl EvaluateMeasureOperation {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EvaluateMeasureOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for EvaluateMeasureOperation {
    fn code(&self) -> &str {
        "evaluate-measure"
    }

    async fn handle_instance(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _resource_type: &str,
        _id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotImplemented(
            "$evaluate-measure population evaluation and scoring are unavailable".into(),
        ))
    }
}
