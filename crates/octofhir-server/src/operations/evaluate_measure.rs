//! Evaluate Measure operation handler.
//!
//! This module implements the `$evaluate-measure` operation which evaluates
//! clinical quality measures using CQL and returns MeasureReport resources.
//!
//! # Supported Report Types
//!
//! - **individual**: Calculate measure for a single subject (patient)
//! - **summary**: Calculate aggregate measure across a population
//!
//! # Examples
//!
//! Individual measure report:
//! ```
//! POST /fhir/Measure/example/$evaluate-measure
//! {
//!   "resourceType": "Parameters",
//!   "parameter": [
//!     { "name": "periodStart", "valueDate": "2024-01-01" },
//!     { "name": "periodEnd", "valueDate": "2024-12-31" },
//!     { "name": "subject", "valueString": "Patient/123" },
//!     { "name": "reportType", "valueCode": "individual" }
//!   ]
//! }
//! ```

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{OperationError, OperationHandler};
use crate::server::AppState;

/// Handler for the `$evaluate-measure` operation.
///
/// Evaluates clinical quality measures and generates MeasureReport resources.
pub struct EvaluateMeasureOperation;

impl EvaluateMeasureOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract date parameter (required)
    fn extract_date_param(&self, params: &Value, name: &str) -> Result<String, OperationError> {
        let parameters = params
            .get("parameter")
            .and_then(|p| p.as_array())
            .ok_or_else(|| {
                OperationError::InvalidParameters("Missing parameter array".to_string())
            })?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some(name) {
                if let Some(date) = param.get("valueDate").and_then(|v| v.as_str()) {
                    return Ok(date.to_string());
                }
                if let Some(datetime) = param.get("valueDateTime").and_then(|v| v.as_str()) {
                    return Ok(datetime.to_string());
                }
            }
        }

        Err(OperationError::InvalidParameters(format!(
            "Missing required parameter: {}",
            name
        )))
    }

    /// Extract string parameter (optional)
    fn extract_string_param(&self, params: &Value, name: &str) -> Option<String> {
        let parameters = params.get("parameter")?.as_array()?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some(name) {
                if let Some(value) = param.get("valueString").and_then(|v| v.as_str()) {
                    return Some(value.to_string());
                }
            }
        }

        None
    }

    /// Extract code parameter (optional, defaults to "individual")
    fn extract_code_param(&self, params: &Value, name: &str) -> Option<String> {
        let parameters = params.get("parameter")?.as_array()?;

        for param in parameters {
            if param.get("name").and_then(|n| n.as_str()) == Some(name) {
                if let Some(value) = param.get("valueCode").and_then(|v| v.as_str()) {
                    return Some(value.to_string());
                }
            }
        }

        None
    }

    /// Build a MeasureReport resource (placeholder for now)
    fn build_measure_report(
        &self,
        measure_url: &str,
        report_type: &str,
        period_start: &str,
        period_end: &str,
        subject: Option<&str>,
    ) -> Value {
        let mut report = json!({
            "resourceType": "MeasureReport",
            "status": "complete",
            "type": report_type,
            "measure": measure_url,
            "period": {
                "start": period_start,
                "end": period_end
            }
        });

        // Add subject reference for individual reports
        if report_type == "individual" {
            if let Some(subject_ref) = subject {
                report["subject"] = json!({
                    "reference": subject_ref
                });
            }
        }

        report
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
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Only supported on Measure resources
        if resource_type != "Measure" {
            return Err(OperationError::NotSupported(
                "$evaluate-measure only supported on Measure resources".to_string(),
            ));
        }

        // Get CQL service from app state
        let _cql_service = state
            .cql_service
            .as_ref()
            .ok_or_else(|| OperationError::NotSupported("CQL service not enabled".to_string()))?;

        // Retrieve Measure resource
        let measure = state
            .storage
            .read("Measure", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| OperationError::NotFound(format!("Measure/{} not found", id)))?;

        // Extract parameters
        let period_start = self.extract_date_param(params, "periodStart")?;
        let period_end = self.extract_date_param(params, "periodEnd")?;
        let report_type = self
            .extract_code_param(params, "reportType")
            .unwrap_or_else(|| "individual".to_string());

        // Extract measure URL
        let measure_url_ref = format!("Measure/{}", id);
        let measure_url = measure
            .resource
            .get("url")
            .and_then(|u: &Value| u.as_str())
            .unwrap_or(&measure_url_ref);

        // For individual report, subject is required
        let subject = if report_type == "individual" {
            Some(
                self.extract_string_param(params, "subject")
                    .ok_or_else(|| {
                        OperationError::InvalidParameters(
                            "subject parameter required for individual reports".to_string(),
                        )
                    })?,
            )
        } else {
            None
        };

        // TODO: Implement actual measure evaluation
        // 1. Load referenced libraries from Measure resource
        // 2. Evaluate CQL definitions based on report type
        // 3. Calculate population counts
        // 4. Build MeasureReport with results

        tracing::warn!(
            measure_id = id,
            report_type = report_type,
            "Measure evaluation not yet fully implemented - returning placeholder"
        );

        // Return placeholder MeasureReport
        Ok(self.build_measure_report(
            measure_url,
            &report_type,
            &period_start,
            &period_end,
            subject.as_deref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_date_param() {
        let operation = EvaluateMeasureOperation::new();

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "periodStart",
                    "valueDate": "2024-01-01"
                }
            ]
        });

        let date = operation
            .extract_date_param(&params, "periodStart")
            .unwrap();
        assert_eq!(date, "2024-01-01");
    }

    #[test]
    fn test_extract_code_param() {
        let operation = EvaluateMeasureOperation::new();

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "reportType",
                    "valueCode": "summary"
                }
            ]
        });

        let code = operation.extract_code_param(&params, "reportType").unwrap();
        assert_eq!(code, "summary");
    }

    #[test]
    fn test_build_measure_report_individual() {
        let operation = EvaluateMeasureOperation::new();

        let report = operation.build_measure_report(
            "http://example.org/Measure/example",
            "individual",
            "2024-01-01",
            "2024-12-31",
            Some("Patient/123"),
        );

        assert_eq!(report["resourceType"], "MeasureReport");
        assert_eq!(report["type"], "individual");
        assert_eq!(report["subject"]["reference"], "Patient/123");
    }

    #[test]
    fn test_build_measure_report_summary() {
        let operation = EvaluateMeasureOperation::new();

        let report = operation.build_measure_report(
            "http://example.org/Measure/example",
            "summary",
            "2024-01-01",
            "2024-12-31",
            None,
        );

        assert_eq!(report["resourceType"], "MeasureReport");
        assert_eq!(report["type"], "summary");
        assert!(report.get("subject").is_none());
    }
}
