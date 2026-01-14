//! CodeSystem $subsumes Operation
//!
//! Implements the FHIR CodeSystem/$subsumes operation to test the subsumption
//! relationship between two codes in a code system.
//!
//! Specification: http://hl7.org/fhir/codesystem-operation-subsumes.html
//!
//! Supported invocation levels:
//! - System: `GET/POST /$subsumes` with system and codes
//! - Type: `GET/POST /CodeSystem/$subsumes` with system/url and codes
//! - Instance: `GET/POST /CodeSystem/{id}/$subsumes` with codes
//!
//! Strategy: Hybrid Approach
//! - For locally loaded CodeSystems (ICD-10, CPT, custom): local hierarchy traversal
//! - For large terminologies (SNOMED CT, LOINC, RxNorm): delegate to tx.fhir.org

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::canonical::get_manager;
use crate::operations::terminology::cache::{CodeSystemKey, get_cache};
use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

// ===== Hardening Constants =====
/// Maximum recursion depth for concept hierarchy traversal
const MAX_RECURSION_DEPTH: usize = 100;

/// Well-known large terminology systems that should be delegated to external server
const LARGE_TERMINOLOGY_SYSTEMS: &[&str] = &[
    "http://snomed.info/sct",
    "http://loinc.org",
    "http://www.nlm.nih.gov/research/umls/rxnorm",
    "urn:oid:2.16.840.1.113883.6.88", // RxNorm OID
];

/// Parameters for the $subsumes operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsumesParams {
    /// Code A to test
    pub code_a: Option<String>,

    /// Code B to test
    pub code_b: Option<String>,

    /// The code system URL
    pub system: Option<String>,

    /// A Coding for code A (alternative to codeA+system)
    #[serde(default)]
    pub coding_a: Option<Value>,

    /// A Coding for code B (alternative to codeB+system)
    #[serde(default)]
    pub coding_b: Option<Value>,

    /// The version of the code system
    pub version: Option<String>,
}

/// The outcome of a subsumption test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubsumptionOutcome {
    /// The codes are equivalent (same code)
    Equivalent,
    /// Code A subsumes code B (A is an ancestor of B)
    Subsumes,
    /// Code A is subsumed by code B (B is an ancestor of A)
    SubsumedBy,
    /// Neither code subsumes the other
    NotSubsumed,
}

impl SubsumptionOutcome {
    /// Convert to FHIR outcome code string.
    pub fn as_code(&self) -> &'static str {
        match self {
            SubsumptionOutcome::Equivalent => "equivalent",
            SubsumptionOutcome::Subsumes => "subsumes",
            SubsumptionOutcome::SubsumedBy => "subsumed-by",
            SubsumptionOutcome::NotSubsumed => "not-subsumed",
        }
    }

    /// Convert to FHIR Parameters resource.
    pub fn to_parameters(&self) -> Value {
        json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "outcome",
                "valueCode": self.as_code()
            }]
        })
    }
}

/// The $subsumes operation handler.
pub struct SubsumesOperation;

impl SubsumesOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<SubsumesParams, OperationError> {
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut subsumes_params = SubsumesParams::default();

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "codeA" => {
                            subsumes_params.code_a = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "codeB" => {
                            subsumes_params.code_b = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "system" => {
                            subsumes_params.system = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "codingA" => {
                            subsumes_params.coding_a = param.get("valueCoding").cloned();
                        }
                        "codingB" => {
                            subsumes_params.coding_b = param.get("valueCoding").cloned();
                        }
                        "version" => {
                            subsumes_params.version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        _ => {}
                    }
                }
            }

            Ok(subsumes_params)
        } else {
            // Assume it's a flat object with query parameters
            let subsumes_params: SubsumesParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok(subsumes_params)
        }
    }

    /// Extract codes and system from params, with coding fallback.
    fn resolve_codes_and_system(
        params: &SubsumesParams,
    ) -> Result<(String, String, String), OperationError> {
        // Extract code A
        let (code_a, system_a) = if let Some(ref coding) = params.coding_a {
            let code = coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from);
            let system = coding
                .get("system")
                .and_then(|v| v.as_str())
                .map(String::from);
            (code, system)
        } else {
            (params.code_a.clone(), params.system.clone())
        };

        // Extract code B
        let (code_b, system_b) = if let Some(ref coding) = params.coding_b {
            let code = coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from);
            let system = coding
                .get("system")
                .and_then(|v| v.as_str())
                .map(String::from);
            (code, system)
        } else {
            (params.code_b.clone(), params.system.clone())
        };

        let code_a = code_a.ok_or_else(|| {
            OperationError::InvalidParameters("Missing 'codeA' or 'codingA' parameter".into())
        })?;

        let code_b = code_b.ok_or_else(|| {
            OperationError::InvalidParameters("Missing 'codeB' or 'codingB' parameter".into())
        })?;

        // Determine system - prefer explicit system, then from codings
        let system = params
            .system
            .clone()
            .or(system_a)
            .or(system_b)
            .ok_or_else(|| {
                OperationError::InvalidParameters(
                    "Missing 'system' parameter or system in codings".into(),
                )
            })?;

        Ok((code_a, code_b, system))
    }

    /// Check if a system is a large terminology that should be delegated.
    fn is_large_terminology(system: &str) -> bool {
        LARGE_TERMINOLOGY_SYSTEMS.iter().any(|s| *s == system)
    }

    /// Load a CodeSystem by URL from cache or canonical manager.
    async fn load_code_system_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Value>, OperationError> {
        let cache = get_cache();
        let cache_key = CodeSystemKey::new(url, version.map(String::from));

        // Check cache first
        if let Some(cached) = cache.get_code_system(&cache_key).await {
            tracing::debug!(url = %url, "CodeSystem loaded from cache (subsumes)");
            return Ok(Some(cached.as_ref().clone()));
        }

        let manager = match get_manager() {
            Some(m) => m,
            None => return Ok(None),
        };

        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("CodeSystem")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .map_err(|e| {
                OperationError::Internal(format!("Failed to search for CodeSystem: {}", e))
            })?;

        let result = search_result
            .resources
            .into_iter()
            .find(|r| {
                let content = &r.resource.content;
                let url_match = content.get("url").and_then(|v| v.as_str()) == Some(url);
                let version_match = match version {
                    Some(ver) => content.get("version").and_then(|v| v.as_str()) == Some(ver),
                    None => true,
                };
                url_match && version_match
            })
            .map(|r| r.resource.content);

        // Cache the result
        cache.insert_code_system(cache_key, result.clone()).await;

        Ok(result)
    }

    /// Load a CodeSystem by ID from storage.
    async fn load_code_system_by_id(
        &self,
        state: &AppState,
        id: &str,
    ) -> Result<Option<Value>, OperationError> {
        let result =
            state.storage.read("CodeSystem", id).await.map_err(|e| {
                OperationError::Internal(format!("Failed to read CodeSystem: {}", e))
            })?;

        match result {
            Some(stored) => Ok(Some(stored.resource)),
            None => {
                // Try canonical manager as fallback
                let manager = get_manager();
                if let Some(mgr) = manager {
                    let search_result = mgr
                        .search()
                        .await
                        .resource_type("CodeSystem")
                        .text(id)
                        .limit(20)
                        .execute()
                        .await
                        .map_err(|e| {
                            OperationError::Internal(format!("Failed to search canonical: {}", e))
                        })?;

                    Ok(search_result
                        .resources
                        .into_iter()
                        .find(|r| r.resource.content.get("id").and_then(|v| v.as_str()) == Some(id))
                        .map(|r| r.resource.content))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Check local subsumption by traversing the concept hierarchy.
    fn check_local_subsumption(
        &self,
        concepts: &[Value],
        code_a: &str,
        code_b: &str,
    ) -> SubsumptionOutcome {
        if code_a == code_b {
            return SubsumptionOutcome::Equivalent;
        }

        // Check if A is an ancestor of B (A subsumes B)
        if self.is_ancestor_of(concepts, code_a, code_b, 0) {
            return SubsumptionOutcome::Subsumes;
        }

        // Check if B is an ancestor of A (A is subsumed by B)
        if self.is_ancestor_of(concepts, code_b, code_a, 0) {
            return SubsumptionOutcome::SubsumedBy;
        }

        SubsumptionOutcome::NotSubsumed
    }

    /// Check if ancestor_code is an ancestor of descendant_code in the hierarchy.
    fn is_ancestor_of(
        &self,
        concepts: &[Value],
        ancestor_code: &str,
        descendant_code: &str,
        depth: usize,
    ) -> bool {
        if depth > MAX_RECURSION_DEPTH {
            tracing::warn!(
                depth = depth,
                ancestor = %ancestor_code,
                descendant = %descendant_code,
                "Maximum recursion depth exceeded in subsumption check"
            );
            return false;
        }

        for concept in concepts {
            let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");

            if code == ancestor_code {
                // Found the potential ancestor, check if descendant is in its subtree
                return self.contains_code_in_subtree(concept, descendant_code, depth + 1);
            }

            // Check children
            if let Some(children) = concept.get("concept").and_then(|v| v.as_array()) {
                if self.is_ancestor_of(children, ancestor_code, descendant_code, depth + 1) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if a code exists anywhere in the subtree of a concept.
    fn contains_code_in_subtree(&self, concept: &Value, target_code: &str, depth: usize) -> bool {
        if depth > MAX_RECURSION_DEPTH {
            return false;
        }

        if let Some(children) = concept.get("concept").and_then(|v| v.as_array()) {
            for child in children {
                let code = child.get("code").and_then(|v| v.as_str()).unwrap_or("");
                if code == target_code {
                    return true;
                }
                if self.contains_code_in_subtree(child, target_code, depth + 1) {
                    return true;
                }
            }
        }

        false
    }

    /// Delegate subsumption check to external terminology server.
    async fn delegate_to_external(
        &self,
        state: &AppState,
        system: &str,
        code_a: &str,
        code_b: &str,
        _version: Option<&str>,
    ) -> Result<SubsumptionOutcome, OperationError> {
        // Use HybridTerminologyProvider for external subsumption checks
        // This integrates with tx.fhir.org for SNOMED CT, LOINC, etc.
        if let Some(ref provider) = state.terminology_provider {
            tracing::debug!(
                system = %system,
                code_a = %code_a,
                code_b = %code_b,
                "Delegating subsumption check to terminology provider"
            );

            // Check if A subsumes B (A is parent of B)
            match provider.subsumes(system, code_a, code_b).await {
                Ok(result) => {
                    use octofhir_fhir_model::terminology::SubsumptionOutcome as FhirSubsumptionOutcome;
                    let outcome = match result.outcome {
                        FhirSubsumptionOutcome::Equivalent => SubsumptionOutcome::Equivalent,
                        FhirSubsumptionOutcome::Subsumes => SubsumptionOutcome::Subsumes,
                        FhirSubsumptionOutcome::SubsumedBy => SubsumptionOutcome::SubsumedBy,
                        FhirSubsumptionOutcome::NotSubsumed => SubsumptionOutcome::NotSubsumed,
                    };
                    tracing::debug!(
                        system = %system,
                        code_a = %code_a,
                        code_b = %code_b,
                        outcome = ?outcome,
                        "Subsumption check completed via terminology provider"
                    );
                    return Ok(outcome);
                }
                Err(e) => {
                    tracing::warn!(
                        system = %system,
                        code_a = %code_a,
                        code_b = %code_b,
                        error = %e,
                        "Terminology provider subsumption check failed, returning not-subsumed"
                    );
                    return Ok(SubsumptionOutcome::NotSubsumed);
                }
            }
        }

        // No terminology provider available - log and return safe default
        tracing::info!(
            system = %system,
            code_a = %code_a,
            code_b = %code_b,
            "Terminology provider not available, cannot check subsumption for external system"
        );

        Ok(SubsumptionOutcome::NotSubsumed)
    }

    /// Perform the subsumption check with hybrid strategy.
    async fn check_subsumption(
        &self,
        state: &AppState,
        system: &str,
        code_a: &str,
        code_b: &str,
        version: Option<&str>,
        code_system_id: Option<&str>,
    ) -> Result<SubsumptionOutcome, OperationError> {
        // Quick check for equivalent codes
        if code_a == code_b {
            return Ok(SubsumptionOutcome::Equivalent);
        }

        // Try to load CodeSystem locally first
        let code_system = if let Some(id) = code_system_id {
            self.load_code_system_by_id(state, id).await?
        } else {
            self.load_code_system_by_url(system, version).await?
        };

        // If we have a local CodeSystem with concepts, use local traversal
        if let Some(ref cs) = code_system {
            if let Some(concepts) = cs.get("concept").and_then(|v| v.as_array()) {
                if !concepts.is_empty() {
                    return Ok(self.check_local_subsumption(concepts, code_a, code_b));
                }
            }
        }

        // For large terminologies or when CodeSystem isn't loaded, delegate to external
        if Self::is_large_terminology(system) || code_system.is_none() {
            return self
                .delegate_to_external(state, system, code_a, code_b, version)
                .await;
        }

        // No concept hierarchy available
        Ok(SubsumptionOutcome::NotSubsumed)
    }
}

impl Default for SubsumesOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for SubsumesOperation {
    fn code(&self) -> &str {
        "subsumes"
    }

    /// Handle system-level $subsumes (GET/POST /$subsumes).
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let subsumes_params = Self::extract_params(params)?;
        let (code_a, code_b, system) = Self::resolve_codes_and_system(&subsumes_params)?;

        self.check_subsumption(
            state,
            &system,
            &code_a,
            &code_b,
            subsumes_params.version.as_deref(),
            None,
        )
        .await
        .map(|outcome| outcome.to_parameters())
    }

    /// Handle type-level $subsumes (GET/POST /CodeSystem/$subsumes).
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "CodeSystem" {
            return Err(OperationError::NotSupported(format!(
                "$subsumes is only supported on CodeSystem, not {}",
                resource_type
            )));
        }

        let subsumes_params = Self::extract_params(params)?;
        let (code_a, code_b, system) = Self::resolve_codes_and_system(&subsumes_params)?;

        self.check_subsumption(
            state,
            &system,
            &code_a,
            &code_b,
            subsumes_params.version.as_deref(),
            None,
        )
        .await
        .map(|outcome| outcome.to_parameters())
    }

    /// Handle instance-level $subsumes (GET/POST /CodeSystem/{id}/$subsumes).
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "CodeSystem" {
            return Err(OperationError::NotSupported(format!(
                "$subsumes is only supported on CodeSystem, not {}",
                resource_type
            )));
        }

        let subsumes_params = Self::extract_params(params)?;

        // For instance-level, we need at least the codes
        let code_a = if let Some(ref coding) = subsumes_params.coding_a {
            coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            subsumes_params.code_a.clone()
        };

        let code_b = if let Some(ref coding) = subsumes_params.coding_b {
            coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            subsumes_params.code_b.clone()
        };

        let code_a = code_a.ok_or_else(|| {
            OperationError::InvalidParameters("Missing 'codeA' or 'codingA' parameter".into())
        })?;

        let code_b = code_b.ok_or_else(|| {
            OperationError::InvalidParameters("Missing 'codeB' or 'codingB' parameter".into())
        })?;

        // System comes from the CodeSystem instance, but we can use it for external fallback
        let system = subsumes_params.system.clone().unwrap_or_default();

        self.check_subsumption(
            state,
            &system,
            &code_a,
            &code_b,
            subsumes_params.version.as_deref(),
            Some(id),
        )
        .await
        .map(|outcome| outcome.to_parameters())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_params_from_query() {
        let params = json!({
            "codeA": "73211009",
            "codeB": "46635009",
            "system": "http://snomed.info/sct"
        });

        let subsumes_params = SubsumesOperation::extract_params(&params).unwrap();
        assert_eq!(subsumes_params.code_a, Some("73211009".to_string()));
        assert_eq!(subsumes_params.code_b, Some("46635009".to_string()));
        assert_eq!(
            subsumes_params.system,
            Some("http://snomed.info/sct".to_string())
        );
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "codeA", "valueCode": "A"},
                {"name": "codeB", "valueCode": "B"},
                {"name": "system", "valueUri": "http://example.org"}
            ]
        });

        let subsumes_params = SubsumesOperation::extract_params(&params).unwrap();
        assert_eq!(subsumes_params.code_a, Some("A".to_string()));
        assert_eq!(subsumes_params.code_b, Some("B".to_string()));
        assert_eq!(
            subsumes_params.system,
            Some("http://example.org".to_string())
        );
    }

    #[test]
    fn test_resolve_codes_and_system() {
        let params = SubsumesParams {
            code_a: Some("A".to_string()),
            code_b: Some("B".to_string()),
            system: Some("http://example.org".to_string()),
            ..Default::default()
        };

        let (code_a, code_b, system) =
            SubsumesOperation::resolve_codes_and_system(&params).unwrap();
        assert_eq!(code_a, "A");
        assert_eq!(code_b, "B");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_resolve_codes_from_codings() {
        let params = SubsumesParams {
            coding_a: Some(json!({"system": "http://example.org", "code": "A"})),
            coding_b: Some(json!({"system": "http://example.org", "code": "B"})),
            ..Default::default()
        };

        let (code_a, code_b, system) =
            SubsumesOperation::resolve_codes_and_system(&params).unwrap();
        assert_eq!(code_a, "A");
        assert_eq!(code_b, "B");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_subsumption_outcome_to_parameters() {
        let outcome = SubsumptionOutcome::Subsumes;
        let params = outcome.to_parameters();

        assert_eq!(params["resourceType"], "Parameters");
        let parameters = params["parameter"].as_array().unwrap();
        assert!(
            parameters
                .iter()
                .any(|p| { p["name"] == "outcome" && p["valueCode"] == "subsumes" })
        );
    }

    #[test]
    fn test_local_subsumption_equivalent() {
        let op = SubsumesOperation::new();
        let concepts = vec![
            json!({"code": "A", "display": "Code A"}),
            json!({"code": "B", "display": "Code B"}),
        ];

        let outcome = op.check_local_subsumption(&concepts, "A", "A");
        assert_eq!(outcome, SubsumptionOutcome::Equivalent);
    }

    #[test]
    fn test_local_subsumption_hierarchy() {
        let op = SubsumesOperation::new();
        let concepts = vec![
            json!({
                "code": "A",
                "display": "Code A",
                "concept": [
                    {"code": "A1", "display": "Code A1"},
                    {"code": "A2", "display": "Code A2", "concept": [
                        {"code": "A2a", "display": "Code A2a"}
                    ]}
                ]
            }),
            json!({"code": "B", "display": "Code B"}),
        ];

        // A should subsume A1 (A is parent of A1)
        let outcome = op.check_local_subsumption(&concepts, "A", "A1");
        assert_eq!(outcome, SubsumptionOutcome::Subsumes);

        // A1 should be subsumed by A
        let outcome = op.check_local_subsumption(&concepts, "A1", "A");
        assert_eq!(outcome, SubsumptionOutcome::SubsumedBy);

        // A should subsume A2a (A is grandparent of A2a)
        let outcome = op.check_local_subsumption(&concepts, "A", "A2a");
        assert_eq!(outcome, SubsumptionOutcome::Subsumes);

        // A and B have no relationship
        let outcome = op.check_local_subsumption(&concepts, "A", "B");
        assert_eq!(outcome, SubsumptionOutcome::NotSubsumed);
    }

    #[test]
    fn test_is_large_terminology() {
        assert!(SubsumesOperation::is_large_terminology(
            "http://snomed.info/sct"
        ));
        assert!(SubsumesOperation::is_large_terminology("http://loinc.org"));
        assert!(!SubsumesOperation::is_large_terminology(
            "http://example.org"
        ));
    }
}
