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
//! Reference resolvers are invocation-local; compiled schemas and FHIRPath
//! plans remain shared across concurrent calls.

use std::sync::Arc;

use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::{
    SchemaProvider,
    reference::{ReferenceResolutionResult, ReferenceResolver, ReferenceResult},
    terminology::TerminologyService,
    types::ValidationError as FhirSchemaValidationError,
    types::ValidationResult,
    validation::FhirValidator,
};

tokio::task_local! {
    static INVOCATION_RESOLVER: Arc<dyn ReferenceResolver>;
}

/// The validator retains this dispatcher while each validation future retains
/// its own resolver. Task-local scope survives yields and cannot mix callers;
/// the validator performs reference work on that future, without spawning it.
struct ScopedReferenceResolver {
    fallback: Arc<dyn ReferenceResolver>,
}

impl ScopedReferenceResolver {
    fn current(&self) -> Arc<dyn ReferenceResolver> {
        INVOCATION_RESOLVER
            .try_with(Arc::clone)
            .unwrap_or_else(|_| self.fallback.clone())
    }
}

#[async_trait::async_trait]
impl ReferenceResolver for ScopedReferenceResolver {
    async fn resource_exists(&self, resource_type: &str, id: &str) -> ReferenceResult<bool> {
        self.current().resource_exists(resource_type, id).await
    }

    async fn resolve_reference(
        &self,
        reference: &str,
    ) -> ReferenceResult<ReferenceResolutionResult> {
        self.current().resolve_reference(reference).await
    }

    async fn fetch_resource(&self, reference: &str) -> ReferenceResult<Option<Arc<JsonValue>>> {
        self.current().fetch_resource(reference).await
    }

    async fn resolve_references_batch(
        &self,
        references: &[String],
    ) -> Vec<ReferenceResult<ReferenceResolutionResult>> {
        self.current().resolve_references_batch(references).await
    }

    async fn fetch_resources_batch(
        &self,
        references: &[String],
    ) -> Vec<ReferenceResult<Option<Arc<JsonValue>>>> {
        self.current().fetch_resources_batch(references).await
    }

    // A declared meta.profile is not proof of conformance. Keep the empty
    // profile map so the validator checks the invocation's actual target body.
}
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
        let mut issues = self
            .issues
            .iter()
            .map(|i| {
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
            })
            .collect::<Vec<_>>();
        if issues.is_empty() {
            issues.push(serde_json::json!({
                "severity": if self.valid { "information" } else { "error" },
                "code": if self.valid { "informational" } else { "invalid" },
                "diagnostics": if self.valid { "Validation completed successfully" } else { "Resource validation failed" }
            }));
        }
        serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": issues
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
    schema_provider: Arc<dyn SchemaProvider>,
}

impl ValidationService {
    /// Package replacement changes compiled schemas as well as model records.
    pub(crate) fn invalidate_caches(&self) {
        self.validator.invalidate_caches();
    }

    /// Create a new validation service with a shared validator.
    ///
    /// Uses `FhirValidator` with `SchemaCompiler` for high-performance
    /// pre-compiled schema validation. Schemas are compiled on first use
    /// and cached for subsequent validations.
    pub fn new(
        model_provider: Arc<OctoFhirModelProvider>,
        fhirpath_engine: Arc<FhirPathEngine>,
    ) -> Self {
        let validator = FhirValidator::new_with_fhirpath(model_provider.clone(), fhirpath_engine);

        Self {
            validator: Arc::new(validator),
            schema_provider: model_provider,
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
        Self::with_options_full(
            model_provider,
            fhirpath_engine,
            reference_resolver,
            terminology_service,
            false,
            5,
        )
    }

    /// Full builder including `targetProfile` conformance validation.
    ///
    /// `check_target_profile` enables dereferencing typed references and
    /// validating the referenced resource against its declared targetProfile(s);
    /// `max_reference_depth` bounds transitive conformance recursion.
    pub fn with_options_full(
        model_provider: Arc<OctoFhirModelProvider>,
        fhirpath_engine: Arc<FhirPathEngine>,
        reference_resolver: Option<Arc<dyn ReferenceResolver>>,
        terminology_service: Option<Arc<dyn TerminologyService>>,
        check_target_profile: bool,
        max_reference_depth: usize,
    ) -> Self {
        let mut validator =
            FhirValidator::new_with_fhirpath(model_provider.clone(), fhirpath_engine);

        if let Some(resolver) = reference_resolver {
            validator = validator
                .with_reference_resolver(Arc::new(ScopedReferenceResolver { fallback: resolver }));
        }

        if let Some(terminology) = terminology_service {
            validator = validator.with_terminology_service(terminology);
        }

        if check_target_profile {
            validator = validator
                .with_target_profile_validation(true)
                .with_max_reference_depth(max_reference_depth);
        }

        Self {
            validator: Arc::new(validator),
            schema_provider: model_provider,
        }
    }

    /// Schemas to validate a resource against: its base type plus every profile
    /// declared in `meta.profile`.
    ///
    /// Server write policy verifies available declared profiles alongside the
    /// base type. FHIR leaves verification of these claims to the server;
    /// unavailable profiles produce warnings and do not fail the resource.
    pub(crate) fn schemas_for(resource: &JsonValue, resource_type: &str) -> Vec<String> {
        let mut schemas = vec![resource_type.to_string()];

        if let Some(profiles) = resource
            .get("meta")
            .and_then(|m| m.get("profile"))
            .and_then(|p| p.as_array())
        {
            schemas.extend(profiles.iter().filter_map(|p| p.as_str()).map(String::from));
        }

        schemas
    }

    /// Validate a resource against its base schema and any declared profiles
    pub async fn validate(&self, resource: &JsonValue) -> ValidationOutcome {
        // Extract resource type
        let resource_type = match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some(rt) => rt,
            None => {
                return ValidationOutcome::error("Missing resourceType".to_string());
            }
        };

        self.run_validation(resource, Self::schemas_for(resource, resource_type), None)
            .await
    }

    /// Validate a resource, treating `known_refs` (`Type/id` strings) as existing.
    ///
    /// Used by transaction Bundle processing: a resource may reference a sibling
    /// created in the same Bundle that is not yet committed to storage. Those
    /// references must not fail existence validation.
    pub async fn validate_with_known_refs(
        &self,
        resource: &JsonValue,
        known_refs: &std::collections::HashSet<String>,
    ) -> ValidationOutcome {
        let resource_type = match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some(rt) => rt,
            None => {
                return ValidationOutcome::error("Missing resourceType".to_string());
            }
        };

        self.run_validation(
            resource,
            Self::schemas_for(resource, resource_type),
            Some(known_refs),
        )
        .await
    }

    async fn run_validation(
        &self,
        resource: &JsonValue,
        schemas: Vec<String>,
        known_refs: Option<&std::collections::HashSet<String>>,
    ) -> ValidationOutcome {
        let base = resource.get("resourceType").and_then(JsonValue::as_str);
        let mut available = Vec::new();
        let mut unavailable = Vec::new();
        for schema in schemas {
            // Preserve hard errors for an unavailable base resource schema.
            // Declared profiles are canonicals, including URNs and relative
            // URIs; `contains("://")` cannot identify their role reliably.
            if Some(schema.as_str()) == base
                || self
                    .schema_provider
                    .get_schema_by_url(&schema)
                    .await
                    .is_some()
            {
                available.push(schema);
            } else {
                unavailable.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    code: "not-supported".into(),
                    diagnostics: format!("Could not resolve declared profile: {schema}"),
                    location: Some("meta.profile".into()),
                });
            }
        }
        let result = self
            .validator
            .validate_with_known_references(resource, available, known_refs)
            .await;
        let mut outcome = Self::convert_result(result);
        outcome.issues.extend(unavailable);
        let constraints = resource_constraints(resource);
        if !constraints.is_empty() {
            outcome.valid = false;
            outcome.issues.extend(constraints);
        }
        outcome
    }

    /// Convert ValidationResult to ValidationOutcome
    fn convert_result(result: ValidationResult) -> ValidationOutcome {
        let issues = result
            .errors
            .iter()
            .map(Self::convert_validation_error)
            .chain(result.warnings.iter().map(|warning| {
                let mut issue = Self::convert_validation_error(warning);
                if warning.constraint_severity.is_none() {
                    issue.severity = IssueSeverity::Warning;
                }
                issue
            }))
            .collect();

        ValidationOutcome {
            valid: result.valid,
            issues,
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
            severity: match error.constraint_severity.as_deref() {
                Some("warning") => IssueSeverity::Warning,
                Some("information") => IssueSeverity::Information,
                _ => IssueSeverity::Error,
            },
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
        self.run_validation(resource, profile_urls.to_vec(), None)
            .await
    }

    pub(crate) async fn validate_profiles_with_resolver(
        &self,
        resource: &JsonValue,
        profiles: &[String],
        known_refs: Option<&std::collections::HashSet<String>>,
        resolver: Arc<dyn ReferenceResolver>,
    ) -> ValidationOutcome {
        INVOCATION_RESOLVER
            .scope(
                resolver,
                self.run_validation(resource, profiles.to_vec(), known_refs),
            )
            .await
    }
}

/// Resource rules that are not encoded as base-schema FHIRPath invariants.
/// Check before writing, so registry installation cannot fail after commit.
pub(crate) fn resource_constraints(resource: &JsonValue) -> Vec<ValidationIssue> {
    if resource["resourceType"] == "SearchParameter"
        && let Some(expression) = resource["expression"].as_str()
        && !octofhir_fhirpath::parse(expression).success
    {
        return vec![ValidationIssue {
            severity: IssueSeverity::Error,
            code: "invalid".into(),
            diagnostics: "SearchParameter.expression must be a valid FHIRPath expression".into(),
            location: Some("SearchParameter.expression".into()),
        }];
    }
    Vec::new()
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

#[cfg(test)]
mod outcome_tests {
    use super::*;

    struct CountedSchemas {
        inner: octofhir_fhirschema::validation::InMemorySchemaProvider,
        compilations: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SchemaProvider for CountedSchemas {
        async fn get_schema(&self, name: &str) -> Option<Arc<octofhir_fhirschema::FhirSchema>> {
            if name == "Observation" {
                self.compilations
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            self.inner.get_schema(name).await
        }

        async fn get_schema_by_url(
            &self,
            url: &str,
        ) -> Option<Arc<octofhir_fhirschema::FhirSchema>> {
            // Compilation loads its root through this method. Count the bare
            // root name, excluding routine target-profile metadata reads.
            if url == "Observation" {
                self.compilations
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            self.inner.get_schema_by_url(url).await
        }
    }

    struct InvocationPatient(bool);

    #[async_trait::async_trait]
    impl ReferenceResolver for InvocationPatient {
        async fn resource_exists(&self, _: &str, _: &str) -> ReferenceResult<bool> {
            Ok(true)
        }

        async fn resolve_reference(&self, _: &str) -> ReferenceResult<ReferenceResolutionResult> {
            Ok(ReferenceResolutionResult::found(
                "Patient".into(),
                "same-target".into(),
            ))
        }

        async fn fetch_resource(&self, _: &str) -> ReferenceResult<Option<Arc<JsonValue>>> {
            tokio::task::yield_now().await;
            let mut body = serde_json::json!({"resourceType":"Patient","id":"same-target"});
            if self.0 {
                body["birthDate"] = serde_json::json!("2000-01-01");
            }
            Ok(Some(Arc::new(body)))
        }
    }

    #[tokio::test]
    async fn concurrent_reference_validation_isolates_callers_and_reuses_compiled_schemas() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut schemas = octofhir_fhirschema::embedded::get_schemas(
            octofhir_fhirschema::embedded::FhirVersion::R4,
        )
        .clone();
        let mut patient = schemas["Patient"].clone();
        patient.url = "urn:test:cached-patient-birth".into();
        patient.name = "CachedPatientBirth".into();
        patient.base = Some("http://hl7.org/fhir/StructureDefinition/Patient".into());
        patient.derivation = Some("constraint".into());
        patient
            .required
            .get_or_insert_with(Vec::new)
            .push("birthDate".into());
        patient
            .elements
            .as_mut()
            .unwrap()
            .get_mut("birthDate")
            .unwrap()
            .min = Some(1);
        let mut observation = schemas["Observation"].clone();
        observation
            .elements
            .as_mut()
            .unwrap()
            .get_mut("subject")
            .unwrap()
            .refers = Some(vec![patient.url.clone()]);
        schemas.insert(patient.url.clone(), patient);
        schemas.insert("Observation".into(), observation);
        let provider = Arc::new(CountedSchemas {
            inner: octofhir_fhirschema::validation::InMemorySchemaProvider::from_map(
                schemas
                    .into_iter()
                    .map(|(name, schema)| (name, Arc::new(schema)))
                    .collect(),
            ),
            compilations: AtomicUsize::new(0),
        });
        let service = Arc::new(ValidationService {
            validator: Arc::new(
                FhirValidator::new(provider.clone())
                    .with_reference_resolver(Arc::new(ScopedReferenceResolver {
                        fallback: Arc::new(InvocationPatient(false)),
                    }))
                    .with_target_profile_validation(true),
            ),
            schema_provider: provider.clone(),
        });
        let body = serde_json::json!({"resourceType":"Observation", "status":"final", "code":{"text":"isolation"},
            "subject":{"reference":"Patient/same-target"}});
        let profiles = vec!["Observation".into()];
        for valid in [false, true] {
            let outcome = service
                .validate_profiles_with_resolver(
                    &body,
                    &profiles,
                    None,
                    Arc::new(InvocationPatient(valid)),
                )
                .await;
            assert_eq!(outcome.valid, valid, "{:?}", outcome.issues);
        }
        let compiled = provider.compilations.load(Ordering::SeqCst);
        assert!(compiled > 0, "the root compilation must be observed");
        let mut tasks = Vec::new();
        for index in 0..16 {
            let service = service.clone();
            let body = body.clone();
            let profiles = profiles.clone();
            tasks.push(tokio::spawn(async move {
                let valid = index % 2 == 0;
                let outcome = service
                    .validate_profiles_with_resolver(
                        &body,
                        &profiles,
                        None,
                        Arc::new(InvocationPatient(valid)),
                    )
                    .await;
                assert_eq!(outcome.valid, valid, "{:?}", outcome.issues);
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(
            provider.compilations.load(Ordering::SeqCst),
            compiled,
            "warm concurrent requests must reuse compiled schemas"
        );
        service.invalidate_caches();
        assert!(
            service
                .validate_profiles_with_resolver(
                    &body,
                    &profiles,
                    None,
                    Arc::new(InvocationPatient(true))
                )
                .await
                .valid
        );
        assert!(
            provider.compilations.load(Ordering::SeqCst) > compiled,
            "package invalidation must discard compiled schemas"
        );
    }

    #[test]
    fn operation_outcome_always_has_at_least_one_issue() {
        let success = ValidationOutcome::success().to_operation_outcome();
        assert_eq!(success["issue"].as_array().unwrap().len(), 1);
        assert_eq!(success["issue"][0]["severity"], "information");
        let failure = ValidationOutcome {
            valid: false,
            issues: Vec::new(),
        }
        .to_operation_outcome();
        assert_eq!(failure["issue"][0]["severity"], "error");
    }

    #[tokio::test]
    async fn validator_warning_stream_is_preserved_in_outcome() {
        let schemas = octofhir_fhirschema::embedded::get_schemas(
            octofhir_fhirschema::embedded::FhirVersion::R4,
        )
        .clone();
        let service = ValidationService {
            validator: Arc::new(FhirValidator::from_schemas(schemas.clone(), None)),
            schema_provider: Arc::new(
                octofhir_fhirschema::validation::InMemorySchemaProvider::from_map(
                    schemas
                        .into_iter()
                        .map(|(name, schema)| (name, Arc::new(schema)))
                        .collect(),
                ),
            ),
        };
        let outcome = service.validate(&serde_json::json!({"resourceType":"Patient", "meta":{"profile":["urn:test:unavailable-profile", "https://example.test/unavailable-profile"]}})).await;
        assert!(outcome.valid);
        assert_eq!(
            outcome
                .issues
                .iter()
                .filter(|issue| issue.severity == IssueSeverity::Warning)
                .count(),
            2
        );
        let upstream = service
            .validator
            .validate(
                &serde_json::json!({"resourceType":"Patient"}),
                vec![
                    "Patient".into(),
                    "https://example.test/unavailable-profile".into(),
                ],
            )
            .await;
        assert!(!upstream.warnings.is_empty());
        assert!(
            ValidationService::convert_result(upstream)
                .issues
                .iter()
                .any(|issue| issue.severity == IssueSeverity::Warning)
        );
        let json = outcome.to_operation_outcome();
        assert!(
            json["issue"].as_array().unwrap().iter().any(|issue| {
                issue["severity"] == "warning"
                    && issue["diagnostics"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("urn:test:unavailable-profile")
            }),
            "{json}"
        );
        assert!(
            !json["issue"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["severity"] == "error"),
            "{json}"
        );
    }
}
