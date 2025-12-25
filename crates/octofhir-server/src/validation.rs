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
//! Schemas are loaded on-demand from the database when validation is requested.

use std::collections::HashMap;
use std::sync::Arc;

use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::{
    reference::ReferenceResolver, types::FhirSchema,
    types::ValidationError as FhirSchemaValidationError, validation::FhirSchemaValidator,
};
use serde_json::Value as JsonValue;
use tracing::warn;

use crate::{canonical, model_provider::OctoFhirModelProvider};

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
/// Combines structural schema validation with FHIRPath constraint evaluation.
/// Schemas are loaded on-demand from the database when validation is requested.
#[derive(Clone)]
pub struct ValidationService {
    /// Model provider for type information and on-demand schema loading
    model_provider: Arc<OctoFhirModelProvider>,
    /// FHIRPath engine for advanced constraint evaluation
    fhirpath_engine: Arc<FhirPathEngine>,
    /// Optional reference resolver for existence validation
    reference_resolver: Option<Arc<dyn ReferenceResolver>>,
}

/// Check if a type is a primitive FHIR type
fn is_primitive_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "boolean"
            | "integer"
            | "string"
            | "decimal"
            | "uri"
            | "url"
            | "canonical"
            | "base64Binary"
            | "instant"
            | "date"
            | "dateTime"
            | "time"
            | "code"
            | "oid"
            | "id"
            | "markdown"
            | "unsignedInt"
            | "positiveInt"
            | "uuid"
            | "xhtml"
    )
}

impl ValidationService {
    /// Create a new validation service with FHIRPath constraint support.
    ///
    /// Schemas are loaded on-demand from the model provider when validation
    /// is requested, rather than pre-loading all schemas at construction.
    pub async fn new(
        model_provider: Arc<OctoFhirModelProvider>,
        fhirpath_engine: Arc<FhirPathEngine>,
    ) -> Result<Self, anyhow::Error> {
        Ok(Self {
            model_provider,
            fhirpath_engine,
            reference_resolver: None,
        })
    }

    /// Add a reference resolver for existence validation.
    ///
    /// When a reference resolver is provided, the validator will check that
    /// referenced resources actually exist in the storage.
    pub fn with_reference_resolver(mut self, resolver: Arc<dyn ReferenceResolver>) -> Self {
        self.reference_resolver = Some(resolver);
        self
    }

    /// Load schemas needed for validating a resource type and its type hierarchy.
    ///
    /// This loads the schema for the given resource type and all its base types
    /// (Resource, DomainResource, etc.) to ensure complete validation.
    async fn load_schemas_for_validation(
        &self,
        resource_type: &str,
    ) -> HashMap<String, FhirSchema> {
        let mut schemas = HashMap::new();
        let mut types_to_load = vec![resource_type.to_string()];
        let mut loaded = std::collections::HashSet::new();

        while let Some(type_name) = types_to_load.pop() {
            if loaded.contains(&type_name) {
                continue;
            }
            loaded.insert(type_name.clone());

            if let Some(schema) = self.model_provider.get_schema(&type_name).await {
                // Add base type to load list
                if let Some(base_url) = &schema.base {
                    if let Some(base_name) = base_url.rsplit('/').next() {
                        if !loaded.contains(base_name) {
                            types_to_load.push(base_name.to_string());
                        }
                    }
                }

                // Load element types recursively
                if let Some(elements) = &schema.elements {
                    for element in elements.values() {
                        if let Some(elem_type) = &element.type_name {
                            if !loaded.contains(elem_type) && !is_primitive_type(elem_type) {
                                types_to_load.push(elem_type.clone());
                            }
                        }
                    }
                }

                schemas.insert(schema.name.clone(), (*schema).clone());
            }
        }

        schemas
    }

    /// Load a specific profile schema by URL for profile validation.
    async fn load_profile_schema(&self, profile_url: &str) -> Option<FhirSchema> {
        self.model_provider
            .get_schema_by_url(profile_url)
            .await
            .map(|s| (*s).clone())
    }

    /// Create a validator with the loaded schemas for a specific validation request.
    async fn create_validator_for_resource(
        &self,
        resource_type: &str,
    ) -> Option<FhirSchemaValidator> {
        let schemas = self.load_schemas_for_validation(resource_type).await;
        if schemas.is_empty() {
            warn!("No schemas loaded for resource type: {}", resource_type);
            return None;
        }
        let mut validator = FhirSchemaValidator::new(schemas, Some(self.fhirpath_engine.clone()));

        // Add reference resolver if configured
        if let Some(resolver) = &self.reference_resolver {
            validator = validator.with_reference_resolver(resolver.clone());
        }

        Some(validator)
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

        // Create validator with on-demand loaded schemas
        let validator = match self.create_validator_for_resource(resource_type).await {
            Some(v) => v,
            None => {
                return ValidationOutcome::error(format!(
                    "Unable to load schemas for resource type: {}",
                    resource_type
                ));
            }
        };

        // Validate against the resource type schema
        let validation_result = validator
            .validate(resource, vec![resource_type.to_string()])
            .await;

        if validation_result.valid {
            ValidationOutcome::success()
        } else {
            let issues = validation_result
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
    /// The profile is loaded from the database by its canonical URL
    /// (as specified in meta.profile).
    pub async fn validate_against_profile(
        &self,
        resource: &JsonValue,
        profile_url: &str,
    ) -> ValidationOutcome {
        // Extract resource type for loading base schemas
        let resource_type = match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some(rt) => rt,
            None => {
                return ValidationOutcome::error("Missing resourceType".to_string());
            }
        };

        // Load schemas including the profile
        let mut schemas = self.load_schemas_for_validation(resource_type).await;

        // Also load the profile schema by URL if different from base
        if let Some(profile_schema) = self.load_profile_schema(profile_url).await {
            schemas.insert(profile_schema.name.clone(), profile_schema);
        } else {
            warn!("Profile not found: {}", profile_url);
            return ValidationOutcome::error(format!("Profile not found: {}", profile_url));
        }

        // Create validator with loaded schemas
        let mut validator = FhirSchemaValidator::new(schemas, Some(self.fhirpath_engine.clone()));

        // Add reference resolver if configured
        if let Some(resolver) = &self.reference_resolver {
            validator = validator.with_reference_resolver(resolver.clone());
        }

        // Validate against the profile
        let validation_result = validator
            .validate(resource, vec![profile_url.to_string()])
            .await;

        if validation_result.valid {
            ValidationOutcome::success()
        } else {
            let issues = validation_result
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
