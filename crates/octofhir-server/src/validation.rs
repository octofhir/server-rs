//! FHIR Resource Validation Service
//!
//! This module provides comprehensive validation for FHIR resources including:
//! - Structural validation using FHIRSchema
//! - FHIRPath constraint evaluation
//! - Profile-based validation
//!
//! # Architecture
//!
//! The ValidationService combines:
//! - `FhirSchemaValidationProvider` for structural schema validation
//! - `FhirPathEvaluator` (via FhirPathEngine) for constraint evaluation
//!
//! These are wired together using the shared traits from `octofhir-fhir-model`.

use std::sync::Arc;

use octofhir_fhir_model::{ValidationProvider, provider::ModelProvider};
use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::create_validation_provider_with_fhirpath;
use serde_json::Value as JsonValue;

#[cfg(test)]
use octofhir_fhirschema::{
    FhirSchemaModelProvider, FhirSchemaValidationProvider, FhirVersion, ValidationContext,
    embedded::get_schemas,
};

use crate::canonical;

/// Validation outcome with detailed information
#[derive(Debug, Clone)]
pub struct ValidationOutcome {
    /// Whether the resource is valid
    pub valid: bool,
    /// List of validation issues
    pub issues: Vec<ValidationIssue>,
}

impl ValidationOutcome {
    /// Create a successful validation outcome
    pub fn success() -> Self {
        Self {
            valid: true,
            issues: Vec::new(),
        }
    }

    /// Create a failed validation outcome with a single error
    pub fn error(message: String) -> Self {
        Self {
            valid: false,
            issues: vec![ValidationIssue {
                severity: IssueSeverity::Error,
                code: "invalid".to_string(),
                diagnostics: message,
                location: None,
            }],
        }
    }

    /// Convert to FHIR OperationOutcome JSON
    pub fn to_operation_outcome(&self) -> JsonValue {
        serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": self.issues.iter().map(|i| {
                let mut issue = serde_json::json!({
                    "severity": i.severity.as_str(),
                    "code": i.code,
                    "diagnostics": i.diagnostics,
                });
                if let Some(loc) = &i.location {
                    issue["location"] = serde_json::json!([loc]);
                }
                issue
            }).collect::<Vec<_>>()
        })
    }
}

/// Single validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Issue severity
    pub severity: IssueSeverity,
    /// Issue code
    pub code: String,
    /// Human-readable diagnostics
    pub diagnostics: String,
    /// Location in the resource (FHIRPath expression)
    pub location: Option<String>,
}

/// Issue severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Fatal error - processing cannot continue
    Fatal,
    /// Error - resource is invalid
    Error,
    /// Warning - resource is valid but has issues
    Warning,
    /// Information - informational note
    Information,
}

impl IssueSeverity {
    /// Convert to FHIR string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueSeverity::Fatal => "fatal",
            IssueSeverity::Error => "error",
            IssueSeverity::Warning => "warning",
            IssueSeverity::Information => "information",
        }
    }
}

/// Comprehensive FHIR validation service
///
/// Combines structural schema validation with FHIRPath constraint evaluation.
#[derive(Clone)]
pub struct ValidationService {
    /// Validation provider for schema + constraint validation
    validation_provider: Arc<dyn ValidationProvider>,
    /// Model provider for type information
    #[allow(dead_code)]
    model_provider: Arc<dyn ModelProvider + Send + Sync>,
    /// FHIRPath engine for advanced constraint evaluation
    #[allow(dead_code)]
    fhirpath_engine: Arc<FhirPathEngine>,
}

impl ValidationService {
    /// Create a new validation service with FHIRPath constraint support
    pub async fn new(
        model_provider: Arc<dyn ModelProvider + Send + Sync>,
        fhirpath_engine: Arc<FhirPathEngine>,
    ) -> Result<Self, anyhow::Error> {
        // Create validation provider with FHIRPath evaluator
        let validation_provider = create_validation_provider_with_fhirpath(
            model_provider.clone(),
            fhirpath_engine.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create validation provider: {}", e))?;

        Ok(Self {
            validation_provider,
            model_provider,
            fhirpath_engine,
        })
    }

    /// Create a validation service without FHIRPath constraint evaluation
    /// (structural validation only)
    ///
    /// **NOTE:** This method creates its own model provider and should only
    /// be used in tests. In production, use `new()` with the shared provider
    /// from AppState.
    #[cfg(test)]
    pub async fn new_structural_only(fhir_version: FhirVersion) -> Result<Self, anyhow::Error> {
        let schemas = get_schemas(fhir_version);
        let model_fhir_version = match fhir_version {
            FhirVersion::R4 => octofhir_fhir_model::FhirVersion::R4,
            FhirVersion::R4B => octofhir_fhir_model::FhirVersion::R4B,
            FhirVersion::R5 => octofhir_fhir_model::FhirVersion::R5,
            FhirVersion::R6 => octofhir_fhir_model::FhirVersion::R6,
        };

        let schema_provider = Arc::new(FhirSchemaModelProvider::new(
            schemas.clone(),
            model_fhir_version,
        ));

        let validation_context = ValidationContext::default();
        let validation_provider = Arc::new(FhirSchemaValidationProvider::new(
            schema_provider.clone(),
            validation_context,
        ));

        // Create FHIRPath engine
        let registry = Arc::new(octofhir_fhirpath::create_function_registry());
        let fhirpath_engine = Arc::new(
            FhirPathEngine::new(registry, schema_provider.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create FHIRPath engine: {}", e))?,
        );

        Ok(Self {
            validation_provider,
            model_provider: schema_provider,
            fhirpath_engine,
        })
    }

    /// Validate a resource against its base schema
    pub async fn validate(&self, resource: &JsonValue) -> ValidationOutcome {
        // Extract resource type
        let resource_type = match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some(rt) => rt,
            None => {
                return ValidationOutcome::error("Missing resourceType".to_string());
            }
        };

        // Build the base profile URL
        let profile_url = format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type);

        self.validate_against_profile(resource, &profile_url).await
    }

    /// Validate a resource against a specific profile
    pub async fn validate_against_profile(
        &self,
        resource: &JsonValue,
        profile_url: &str,
    ) -> ValidationOutcome {
        match self
            .validation_provider
            .validate(resource, profile_url)
            .await
        {
            Ok(true) => ValidationOutcome::success(),
            Ok(false) => ValidationOutcome {
                valid: false,
                issues: vec![ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "invalid".to_string(),
                    diagnostics: format!("Resource does not conform to profile: {}", profile_url),
                    location: None,
                }],
            },
            Err(e) => ValidationOutcome::error(format!("Validation error: {}", e)),
        }
    }

    /// Validate a resource against multiple profiles
    pub async fn validate_against_profiles(
        &self,
        resource: &JsonValue,
        profile_urls: &[String],
    ) -> ValidationOutcome {
        let mut all_issues = Vec::new();
        let mut all_valid = true;

        for profile_url in profile_urls {
            let outcome = self.validate_against_profile(resource, profile_url).await;
            if !outcome.valid {
                all_valid = false;
            }
            all_issues.extend(outcome.issues);
        }

        ValidationOutcome {
            valid: all_valid,
            issues: all_issues,
        }
    }
}

/// Placeholder for resource validation that can access the canonical registry.
/// This will evolve to real profile/StructureDefinition validation.
pub fn validate_resource(resource_type: &str, body: &JsonValue) -> Result<(), String> {
    // Demonstrate registry access for acceptance: read packages for potential rules
    let _pkg_count = canonical::with_registry(|r| r.list().len()).unwrap_or(0);

    // Minimal shape checks for MVP
    let obj = body
        .as_object()
        .ok_or_else(|| "body must be a JSON object".to_string())?;
    let rt = obj
        .get("resourceType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing resourceType".to_string())?;
    if rt != resource_type {
        return Err(format!(
            "resourceType '{rt}' does not match path '{resource_type}'"
        ));
    }
    Ok(())
}
