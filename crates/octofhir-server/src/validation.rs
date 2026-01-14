//! FHIR Resource Validation Service
//!
//! This module provides comprehensive validation for FHIR resources including:
//! - Structural validation using FHIRSchema
//! - FHIRPath constraint evaluation
//! - Profile-based validation
//!
//! # Architecture
//!
//! The ValidationService uses a single shared `FhirSchemaValidator` instance
//! that lazily loads schemas via the `SchemaProvider` trait. This avoids
//! creating new validators per request, significantly improving performance.
//!
//! Schemas are loaded on-demand from the `OctoFhirModelProvider` which has
//! an internal Moka LRU cache for efficient schema reuse.

use std::sync::Arc;

use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::{
    reference::ReferenceResolver, terminology::TerminologyService,
    types::ValidationError as FhirSchemaValidationError, types::ValidationResult,
    validation::FhirValidator,
};
use serde_json::Value as JsonValue;

use crate::model_provider::OctoFhirModelProvider;

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
                    issue["expression"] = serde_json::json!([loc]);
                }
                // Add details for reference errors
                if i.code.starts_with("REF") || i.code == "FS1013" {
                    let detail_code = if i.code == "REF1001" {
                        "non-existent-resource"
                    } else if i.code == "REF1002" {
                        "contained-not-found"
                    } else if i.code == "FS1013" {
                        "invalid-reference-type"
                    } else {
                        "reference-error"
                    };
                    issue["details"] = serde_json::json!({
                        "coding": [{
                            "system": "http://octofhir.io/CodeSystem/operation-outcome-type",
                            "code": detail_code
                        }]
                    });
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
/// Uses `FhirValidator` with pre-compiled schemas for high-performance validation.
/// Schemas are lazily compiled on first use and cached by the `SchemaCompiler`.
#[derive(Clone)]
pub struct ValidationService {
    /// Shared validator instance with pre-compiled schemas
    validator: Arc<FhirValidator>,
}

impl ValidationService {
    /// Create a new validation service with a shared validator.
    ///
    /// Uses `FhirValidator` with `SchemaCompiler` for high-performance
    /// pre-compiled schema validation. Schemas are compiled on first use
    /// and cached for subsequent validations.
    pub fn new(
        model_provider: Arc<OctoFhirModelProvider>,
        fhirpath_engine: Arc<FhirPathEngine>,
    ) -> Self {
        let validator = FhirValidator::new_with_fhirpath(model_provider, fhirpath_engine);

        Self {
            validator: Arc::new(validator),
        }
    }

    /// Create a new validation service with additional configuration.
    ///
    /// This builder-style constructor allows adding reference resolver and
    /// terminology service to the shared validator.
    pub fn with_options(
        model_provider: Arc<OctoFhirModelProvider>,
        fhirpath_engine: Arc<FhirPathEngine>,
        reference_resolver: Option<Arc<dyn ReferenceResolver>>,
        terminology_service: Option<Arc<dyn TerminologyService>>,
    ) -> Self {
        let mut validator = FhirValidator::new_with_fhirpath(model_provider, fhirpath_engine);

        if let Some(resolver) = reference_resolver {
            validator = validator.with_reference_resolver(resolver);
        }

        if let Some(terminology) = terminology_service {
            validator = validator.with_terminology_service(terminology);
        }

        Self {
            validator: Arc::new(validator),
        }
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

        // Validate using shared validator (schemas loaded lazily via SchemaProvider)
        let validation_result = self
            .validator
            .validate(resource, vec![resource_type.to_string()])
            .await;

        Self::convert_result(validation_result)
    }

    /// Convert ValidationResult to ValidationOutcome
    fn convert_result(result: ValidationResult) -> ValidationOutcome {
        if result.valid {
            ValidationOutcome::success()
        } else {
            let issues = result
                .errors
                .iter()
                .map(Self::convert_validation_error)
                .collect();

            ValidationOutcome {
                valid: false,
                issues,
            }
        }
    }

    /// Convert FHIR Schema validation error to ValidationIssue
    fn convert_validation_error(error: &FhirSchemaValidationError) -> ValidationIssue {
        let diagnostics = if let Some(msg) = &error.message {
            msg.clone()
        } else {
            format!("Validation error: {}", error.error_type)
        };

        // Build a more detailed diagnostic message
        let detailed_diagnostics = if let Some(expected) = &error.expected {
            format!("{} (expected: {})", diagnostics, expected)
        } else if let Some(got) = &error.got {
            format!("{} (got: {})", diagnostics, got)
        } else {
            diagnostics
        };

        // Convert path to FHIRPath location string
        let location = if !error.path.is_empty() {
            Some(
                error
                    .path
                    .iter()
                    .map(|v| match v {
                        JsonValue::String(s) => s.clone(),
                        JsonValue::Number(n) => format!("[{}]", n),
                        _ => v.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("."),
            )
        } else {
            None
        };

        ValidationIssue {
            severity: IssueSeverity::Error,
            code: "invalid".to_string(),
            diagnostics: detailed_diagnostics,
            location,
        }
    }

    /// Validate a resource against a specific profile URL
    ///
    /// The profile is loaded lazily via the SchemaProvider when needed.
    pub async fn validate_against_profile(
        &self,
        resource: &JsonValue,
        profile_url: &str,
    ) -> ValidationOutcome {
        // Validate using shared validator with profile URL
        // The validator will lazily load the profile schema via SchemaProvider
        let validation_result = self
            .validator
            .validate(resource, vec![profile_url.to_string()])
            .await;

        Self::convert_result(validation_result)
    }

    /// Validate a resource against multiple profiles
    pub async fn validate_against_profiles(
        &self,
        resource: &JsonValue,
        profile_urls: &[String],
    ) -> ValidationOutcome {
        // Validate using shared validator with all profile URLs
        // The validator will lazily load all profile schemas via SchemaProvider
        let validation_result = self
            .validator
            .validate(resource, profile_urls.to_vec())
            .await;

        Self::convert_result(validation_result)
    }
}

/// Placeholder for resource validation using cached resource types.
/// This will evolve to real profile/StructureDefinition validation.
pub fn validate_resource(
    resource_type: &str,
    body: &JsonValue,
    known_resource_types: &std::collections::HashSet<String>,
) -> Result<(), String> {
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

    if !known_resource_types.is_empty() && !known_resource_types.contains(rt) {
        return Err(format!("Unknown resourceType '{rt}'"));
    }

    if known_resource_types.is_empty() && !octofhir_core::fhir::is_valid_resource_type_name(rt) {
        return Err(format!("Invalid resourceType '{rt}'"));
    }

    Ok(())
}
