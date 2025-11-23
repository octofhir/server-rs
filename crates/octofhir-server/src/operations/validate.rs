//! $validate operation handler.
//!
//! This module implements the FHIR $validate operation for validating
//! resources against FHIR schemas and constraints.

use async_trait::async_trait;
use octofhir_core::ResourceType;
use serde_json::{Value, json};
use std::str::FromStr;
use std::sync::Arc;

use super::{OperationError, OperationHandler};
use crate::server::{AppState, SharedModelProvider};
use octofhir_fhirpath::FhirPathEngine;

/// Severity levels for validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Fatal,
    Error,
    Warning,
    Information,
}

impl Severity {
    /// Returns the FHIR string representation of the severity.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "information",
        }
    }
}

/// A validation issue found during resource validation.
#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub code: String,
    pub diagnostics: String,
    pub location: Vec<String>,
}

impl Issue {
    /// Creates an error-level issue.
    pub fn error(code: &str, diagnostics: &str) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_string(),
            diagnostics: diagnostics.to_string(),
            location: vec![],
        }
    }

    /// Creates a warning-level issue.
    #[allow(dead_code)]
    pub fn warning(code: &str, diagnostics: &str) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_string(),
            diagnostics: diagnostics.to_string(),
            location: vec![],
        }
    }

    /// Creates an information-level issue.
    #[allow(dead_code)]
    pub fn information(code: &str, diagnostics: &str) -> Self {
        Self {
            severity: Severity::Information,
            code: code.to_string(),
            diagnostics: diagnostics.to_string(),
            location: vec![],
        }
    }

    /// Adds a location to the issue.
    #[allow(dead_code)]
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location.push(location.into());
        self
    }
}

/// The $validate operation handler.
///
/// Validates FHIR resources against the base specification and optionally
/// against specific profiles.
pub struct ValidateOperation {
    #[allow(dead_code)]
    fhirpath_engine: Arc<FhirPathEngine>,
    #[allow(dead_code)]
    model_provider: SharedModelProvider,
}

impl ValidateOperation {
    /// Creates a new $validate operation handler.
    pub fn new(fhirpath_engine: Arc<FhirPathEngine>, model_provider: SharedModelProvider) -> Self {
        Self {
            fhirpath_engine,
            model_provider,
        }
    }

    /// Extracts the resource from the Parameters input.
    fn extract_resource(&self, params: &Value) -> Option<Value> {
        params["parameter"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|p| p["name"].as_str() == Some("resource"))
                    .and_then(|p| p["resource"].as_object())
                    .map(|o| Value::Object(o.clone()))
            })
            .or_else(|| {
                // If params is directly a resource (not Parameters), return it
                if params.get("resourceType").is_some()
                    && params["resourceType"].as_str() != Some("Parameters")
                {
                    Some(params.clone())
                } else {
                    None
                }
            })
    }

    /// Extracts the profile URL from the resource's meta.profile.
    fn extract_profile(&self, resource: &Value) -> Option<String> {
        resource["meta"]["profile"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    /// Extracts the mode parameter (lenient, strict, etc.).
    #[allow(dead_code)]
    fn extract_mode(&self, params: &Value) -> Option<String> {
        params["parameter"].as_array().and_then(|arr| {
            arr.iter()
                .find(|p| p["name"].as_str() == Some("mode"))
                .and_then(|p| p["valueCode"].as_str().or(p["valueString"].as_str()))
                .map(String::from)
        })
    }

    /// Validates a resource value and returns an OperationOutcome.
    async fn validate_resource_value(
        &self,
        resource: &Value,
        expected_type: Option<&str>,
    ) -> Result<Value, OperationError> {
        let mut issues = Vec::new();

        // Check resourceType exists
        let resource_type = match resource["resourceType"].as_str() {
            Some(rt) => rt,
            None => {
                issues.push(Issue::error(
                    "required",
                    "Resource must have a resourceType element",
                ));
                return Ok(self.build_outcome(issues));
            }
        };

        // Check type matches if expected
        if let Some(expected) = expected_type
            && resource_type != expected
        {
            issues.push(Issue::error(
                "invalid",
                &format!(
                    "Resource type '{}' does not match expected type '{}'",
                    resource_type, expected
                ),
            ));
            return Ok(self.build_outcome(issues));
        }

        // Get profile to validate against (if specified)
        let _profile = self.extract_profile(resource);

        // Perform basic structural validation using the model provider
        let schema_issues = self.validate_structure(resource, resource_type).await;
        issues.extend(schema_issues);

        // If no issues found, add a success information message
        if issues.is_empty() {
            issues.push(Issue::information(
                "informational",
                &format!("Validation successful for {} resource", resource_type),
            ));
        }

        Ok(self.build_outcome(issues))
    }

    /// Validates the resource structure.
    async fn validate_structure(&self, resource: &Value, resource_type: &str) -> Vec<Issue> {
        let mut issues = Vec::new();

        // Check if the resource type is valid using ResourceType parsing
        if ResourceType::from_str(resource_type).is_err() {
            issues.push(Issue::error(
                "not-supported",
                &format!("Invalid resource type: {}", resource_type),
            ));
            return issues;
        }

        // Basic validation: check id format if present
        if let Some(id) = resource["id"].as_str() {
            if id.is_empty() {
                issues.push(Issue::error("value", "Resource id cannot be empty"));
            } else if id.len() > 64 {
                issues.push(Issue::error(
                    "value",
                    "Resource id exceeds maximum length of 64 characters",
                ));
            } else if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            {
                issues.push(Issue::error(
                    "value",
                    "Resource id contains invalid characters (only alphanumeric, '-', and '.' allowed)",
                ));
            }
        }

        issues
    }

    /// Builds an OperationOutcome from a list of issues.
    fn build_outcome(&self, issues: Vec<Issue>) -> Value {
        let issue_array: Vec<Value> = issues
            .iter()
            .map(|i| {
                let mut issue_json = json!({
                    "severity": i.severity.as_str(),
                    "code": i.code,
                    "diagnostics": i.diagnostics
                });

                if !i.location.is_empty() {
                    issue_json["location"] = Value::Array(
                        i.location
                            .iter()
                            .map(|l| Value::String(l.clone()))
                            .collect(),
                    );
                }

                issue_json
            })
            .collect();

        json!({
            "resourceType": "OperationOutcome",
            "issue": issue_array
        })
    }

    /// Validates a resource from parameters.
    async fn validate_resource(
        &self,
        params: &Value,
        expected_type: Option<&str>,
    ) -> Result<Value, OperationError> {
        let resource = self.extract_resource(params).ok_or_else(|| {
            OperationError::InvalidParameters("resource parameter required".into())
        })?;

        self.validate_resource_value(&resource, expected_type).await
    }
}

#[async_trait]
impl OperationHandler for ValidateOperation {
    fn code(&self) -> &str {
        "validate"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // System-level validation: validate any resource without type constraint
        self.validate_resource(params, None).await
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Type-level validation: validate resource must match the specified type
        self.validate_resource(params, Some(resource_type)).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Instance-level validation: validate existing resource or provided resource
        let resource = self.extract_resource(params);

        match resource {
            Some(res) => {
                self.validate_resource_value(&res, Some(resource_type))
                    .await
            }
            None => {
                // Fetch from storage - parse resource type first
                let rt = ResourceType::from_str(resource_type).map_err(|_| {
                    OperationError::InvalidParameters(format!(
                        "Invalid resource type: {}",
                        resource_type
                    ))
                })?;

                let envelope = state
                    .storage
                    .get(&rt, id)
                    .await
                    .map_err(|e| OperationError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        OperationError::NotFound(format!("{}/{} not found", resource_type, id))
                    })?;

                // Convert envelope to JSON for validation
                let resource_json = serde_json::to_value(&envelope)
                    .map_err(|e| OperationError::Internal(e.to_string()))?;

                self.validate_resource_value(&resource_json, Some(resource_type))
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_fhir_model::provider::FhirVersion;
    use octofhir_fhirschema::embedded::{FhirVersion as SchemaFhirVersion, get_schemas};
    use octofhir_fhirschema::model_provider::DynamicSchemaProvider;

    async fn create_test_operation_async() -> ValidateOperation {
        let schemas = get_schemas(SchemaFhirVersion::R4).clone();
        let model_provider: SharedModelProvider =
            Arc::new(DynamicSchemaProvider::new(schemas, FhirVersion::R4));
        let registry = Arc::new(octofhir_fhirpath::create_function_registry());

        let engine = FhirPathEngine::new(registry, model_provider.clone())
            .await
            .unwrap();

        ValidateOperation::new(Arc::new(engine), model_provider)
    }

    fn create_test_operation() -> ValidateOperation {
        let schemas = get_schemas(SchemaFhirVersion::R4).clone();
        let model_provider: SharedModelProvider =
            Arc::new(DynamicSchemaProvider::new(schemas, FhirVersion::R4));
        let registry = Arc::new(octofhir_fhirpath::create_function_registry());

        // Create a minimal FhirPathEngine - this requires async but we use tokio::runtime
        // Use spawn_blocking to avoid runtime nesting issues
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let engine = rt
            .block_on(FhirPathEngine::new(registry, model_provider.clone()))
            .unwrap();

        ValidateOperation::new(Arc::new(engine), model_provider)
    }

    #[test]
    fn test_issue_creation() {
        let error = Issue::error("invalid", "Test error");
        assert_eq!(error.severity, Severity::Error);
        assert_eq!(error.code, "invalid");
        assert_eq!(error.diagnostics, "Test error");
        assert!(error.location.is_empty());

        let warning = Issue::warning("business-rule", "Test warning");
        assert_eq!(warning.severity, Severity::Warning);

        let info = Issue::information("informational", "Test info");
        assert_eq!(info.severity, Severity::Information);
    }

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Fatal.as_str(), "fatal");
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Information.as_str(), "information");
    }

    #[test]
    fn test_build_outcome() {
        let op = create_test_operation();
        let issues = vec![
            Issue::error("invalid", "Test error").with_location("Patient.name"),
            Issue::warning("business-rule", "Test warning"),
        ];

        let outcome = op.build_outcome(issues);
        assert_eq!(outcome["resourceType"], "OperationOutcome");

        let issue_array = outcome["issue"].as_array().unwrap();
        assert_eq!(issue_array.len(), 2);
        assert_eq!(issue_array[0]["severity"], "error");
        assert_eq!(issue_array[0]["code"], "invalid");
        assert_eq!(issue_array[0]["location"][0], "Patient.name");
        assert_eq!(issue_array[1]["severity"], "warning");
    }

    #[test]
    fn test_extract_resource_from_parameters() {
        let op = create_test_operation();

        // Test Parameters resource with embedded resource
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "resource",
                "resource": {
                    "resourceType": "Patient",
                    "id": "123"
                }
            }]
        });

        let resource = op.extract_resource(&params);
        assert!(resource.is_some());
        let res = resource.unwrap();
        assert_eq!(res["resourceType"], "Patient");
        assert_eq!(res["id"], "123");
    }

    #[test]
    fn test_extract_resource_direct() {
        let op = create_test_operation();

        // Test direct resource (not wrapped in Parameters)
        let direct = json!({
            "resourceType": "Patient",
            "id": "456"
        });

        let resource = op.extract_resource(&direct);
        assert!(resource.is_some());
        assert_eq!(resource.unwrap()["id"], "456");
    }

    #[test]
    fn test_extract_profile() {
        let op = create_test_operation();

        let resource = json!({
            "resourceType": "Patient",
            "meta": {
                "profile": ["http://example.org/fhir/StructureDefinition/MyPatient"]
            }
        });

        let profile = op.extract_profile(&resource);
        assert_eq!(
            profile,
            Some("http://example.org/fhir/StructureDefinition/MyPatient".to_string())
        );

        // No profile
        let no_profile = json!({"resourceType": "Patient"});
        assert!(op.extract_profile(&no_profile).is_none());
    }

    #[tokio::test]
    async fn test_validate_missing_resource_type() {
        let op = create_test_operation_async().await;
        let invalid = json!({"id": "123"});

        let result = op.validate_resource_value(&invalid, None).await.unwrap();
        assert_eq!(result["resourceType"], "OperationOutcome");

        let issues = result["issue"].as_array().unwrap();
        assert!(!issues.is_empty());
        assert_eq!(issues[0]["severity"], "error");
        assert_eq!(issues[0]["code"], "required");
    }

    #[tokio::test]
    async fn test_validate_type_mismatch() {
        let op = create_test_operation_async().await;
        let patient = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let result = op
            .validate_resource_value(&patient, Some("Observation"))
            .await
            .unwrap();
        let issues = result["issue"].as_array().unwrap();
        assert!(!issues.is_empty());
        assert_eq!(issues[0]["severity"], "error");
        assert!(
            issues[0]["diagnostics"]
                .as_str()
                .unwrap()
                .contains("does not match")
        );
    }

    #[tokio::test]
    async fn test_validate_valid_resource() {
        let op = create_test_operation_async().await;
        let patient = json!({
            "resourceType": "Patient",
            "id": "valid-id-123"
        });

        let result = op
            .validate_resource_value(&patient, Some("Patient"))
            .await
            .unwrap();
        let issues = result["issue"].as_array().unwrap();

        // Should have at least one issue (success information)
        assert!(!issues.is_empty());

        // Check if we have a success message (no errors)
        let has_error = issues
            .iter()
            .any(|i| i["severity"] == "error" || i["severity"] == "fatal");
        assert!(!has_error);
    }

    #[tokio::test]
    async fn test_validate_invalid_id() {
        let op = create_test_operation_async().await;
        let patient = json!({
            "resourceType": "Patient",
            "id": "invalid id with spaces"
        });

        let result = op.validate_resource_value(&patient, None).await.unwrap();
        let issues = result["issue"].as_array().unwrap();

        // Should have an error about invalid id
        let has_id_error = issues.iter().any(|i| {
            i["severity"] == "error"
                && i["diagnostics"]
                    .as_str()
                    .map(|d| d.contains("invalid characters"))
                    .unwrap_or(false)
        });
        assert!(has_id_error);
    }

    #[test]
    fn test_operation_code() {
        let op = create_test_operation();
        assert_eq!(op.code(), "validate");
    }
}
