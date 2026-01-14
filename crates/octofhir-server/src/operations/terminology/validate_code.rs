//! CodeSystem/$validate-code and ValueSet/$validate-code Operations
//!
//! Implements the FHIR $validate-code operation for both CodeSystem and ValueSet.
//!
//! Specifications:
//! - CodeSystem: http://hl7.org/fhir/codesystem-operation-validate-code.html
//! - ValueSet: http://hl7.org/fhir/valueset-operation-validate-code.html
//!
//! Supported invocation levels:
//! - System: `POST /$validate-code` with url or coding
//! - Type: `GET/POST /CodeSystem/$validate-code` or `/ValueSet/$validate-code`
//! - Instance: `GET/POST /CodeSystem/{id}/$validate-code` or `/ValueSet/{id}/$validate-code`

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::canonical::get_manager;
use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

// ===== Hardening Constants =====
/// Maximum recursion depth for concept hierarchy traversal
const MAX_RECURSION_DEPTH: usize = 100;

/// Parameters for the $validate-code operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateCodeParams {
    /// Canonical URL of the CodeSystem or ValueSet
    pub url: Option<String>,

    /// The code to validate
    pub code: Option<String>,

    /// The code system for the code (required for ValueSet validation)
    pub system: Option<String>,

    /// The version of the code system
    pub system_version: Option<String>,

    /// The display text to validate
    pub display: Option<String>,

    /// A Coding to validate (alternative to code+system)
    #[serde(default)]
    pub coding: Option<Value>,

    /// A CodeableConcept to validate (alternative to code+system)
    #[serde(default)]
    pub codeable_concept: Option<Value>,

    /// If true, infer the system from the ValueSet definition
    /// (when only one system is included in the ValueSet)
    #[serde(default)]
    pub infer_system: Option<bool>,

    /// If true, abstract codes are allowed (default: false)
    /// Abstract codes are used for grouping and normally cannot be selected
    #[serde(default)]
    pub abstract_allowed: Option<bool>,
}

/// Result of code validation.
#[derive(Debug, Clone, Serialize)]
pub struct ValidateCodeResult {
    /// Whether the code is valid
    pub result: bool,
    /// Optional message explaining the result
    pub message: Option<String>,
    /// Display text for the code (if found)
    pub display: Option<String>,
}

impl ValidateCodeResult {
    /// Create a valid result
    pub fn valid(display: Option<String>) -> Self {
        Self {
            result: true,
            message: None,
            display,
        }
    }

    /// Create an invalid result with a message
    pub fn invalid(message: String) -> Self {
        Self {
            result: false,
            message: Some(message),
            display: None,
        }
    }

    /// Convert to FHIR Parameters resource
    pub fn to_parameters(&self) -> Value {
        let mut params = vec![json!({
            "name": "result",
            "valueBoolean": self.result
        })];

        if let Some(ref msg) = self.message {
            params.push(json!({
                "name": "message",
                "valueString": msg
            }));
        }

        if let Some(ref display) = self.display {
            params.push(json!({
                "name": "display",
                "valueString": display
            }));
        }

        json!({
            "resourceType": "Parameters",
            "parameter": params
        })
    }
}

/// The $validate-code operation handler.
pub struct ValidateCodeOperation;

impl ValidateCodeOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<ValidateCodeParams, OperationError> {
        // Check if this is a Parameters resource
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut validate_params = ValidateCodeParams::default();

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "url" => {
                            validate_params.url = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "code" => {
                            validate_params.code = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "system" => {
                            validate_params.system = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "systemVersion" => {
                            validate_params.system_version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "display" => {
                            validate_params.display = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "coding" => {
                            validate_params.coding = param.get("valueCoding").cloned();
                        }
                        "codeableConcept" => {
                            validate_params.codeable_concept =
                                param.get("valueCodeableConcept").cloned();
                        }
                        "inferSystem" => {
                            validate_params.infer_system =
                                param.get("valueBoolean").and_then(|v| v.as_bool());
                        }
                        "abstractAllowed" | "abstract" => {
                            validate_params.abstract_allowed =
                                param.get("valueBoolean").and_then(|v| v.as_bool());
                        }
                        _ => {}
                    }
                }
            }

            Ok(validate_params)
        } else {
            // Assume it's a flat object with query parameters
            let validate_params: ValidateCodeParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok(validate_params)
        }
    }

    /// Validate a code against a CodeSystem.
    async fn validate_code_system(
        &self,
        state: &AppState,
        code_system_url: Option<&str>,
        code_system_id: Option<&str>,
        code: &str,
        display: Option<&str>,
        abstract_allowed: bool,
    ) -> Result<ValidateCodeResult, OperationError> {
        // Load the CodeSystem
        let code_system = if let Some(id) = code_system_id {
            self.load_code_system_by_id(state, id).await?
        } else if let Some(url) = code_system_url {
            self.load_code_system_by_url(url).await?
        } else {
            return Err(OperationError::InvalidParameters(
                "Either 'url' parameter or CodeSystem instance ID is required".into(),
            ));
        };

        // Search for the code in the CodeSystem's concepts
        let concepts = code_system
            .get("concept")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Recursively search for the code
        if let Some(found) = self.find_concept_in_hierarchy(&concepts, code) {
            let concept_display = found.get("display").and_then(|v| v.as_str());

            // Check if the code is abstract
            if !abstract_allowed && Self::is_abstract_concept(found) {
                return Ok(ValidateCodeResult {
                    result: false,
                    message: Some(format!(
                        "Code '{}' is abstract and cannot be used for data entry. Set 'abstractAllowed=true' to allow abstract codes.",
                        code
                    )),
                    display: concept_display.map(String::from),
                });
            }

            // If display was provided, validate it matches
            if let Some(expected_display) = display
                && let Some(actual_display) = concept_display
                && actual_display != expected_display
            {
                return Ok(ValidateCodeResult {
                    result: true, // Code is valid, but display doesn't match
                    message: Some(format!(
                        "Code '{}' is valid, but display '{}' does not match expected '{}'",
                        code, actual_display, expected_display
                    )),
                    display: Some(actual_display.to_string()),
                });
            }

            Ok(ValidateCodeResult::valid(concept_display.map(String::from)))
        } else {
            let cs_name = code_system
                .get("name")
                .or_else(|| code_system.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            Ok(ValidateCodeResult::invalid(format!(
                "Code '{}' not found in CodeSystem '{}'",
                code, cs_name
            )))
        }
    }

    /// Validate a code against a ValueSet.
    async fn validate_value_set(
        &self,
        state: &AppState,
        value_set_url: Option<&str>,
        value_set_id: Option<&str>,
        code: &str,
        system: Option<&str>,
        display: Option<&str>,
        infer_system: bool,
        abstract_allowed: bool,
    ) -> Result<ValidateCodeResult, OperationError> {
        // Load the ValueSet
        let value_set = if let Some(id) = value_set_id {
            self.load_value_set_by_id(state, id).await?
        } else if let Some(url) = value_set_url {
            self.load_value_set_by_url(url).await?
        } else {
            return Err(OperationError::InvalidParameters(
                "Either 'url' parameter or ValueSet instance ID is required".into(),
            ));
        };

        // Try to infer system if not provided and inferSystem is true
        let effective_system = if system.is_none() && infer_system {
            self.infer_system_from_valueset(&value_set)
        } else {
            system.map(String::from)
        };

        // First check if ValueSet has an existing expansion
        if let Some(expansion) = value_set.get("expansion") {
            return self.validate_in_expansion(
                expansion,
                code,
                effective_system.as_deref(),
                display,
                abstract_allowed,
            );
        }

        // No expansion, need to process compose
        if let Some(compose) = value_set.get("compose") {
            return self
                .validate_in_compose(
                    compose,
                    code,
                    effective_system.as_deref(),
                    display,
                    abstract_allowed,
                )
                .await;
        }

        // No compose or expansion
        Ok(ValidateCodeResult::invalid(
            "ValueSet has no expansion or compose definition".into(),
        ))
    }

    /// Validate code against an existing expansion.
    fn validate_in_expansion(
        &self,
        expansion: &Value,
        code: &str,
        system: Option<&str>,
        display: Option<&str>,
        abstract_allowed: bool,
    ) -> Result<ValidateCodeResult, OperationError> {
        let contains = expansion
            .get("contains")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        for entry in &contains {
            let entry_code = entry.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let entry_system = entry.get("system").and_then(|v| v.as_str());
            let entry_display = entry.get("display").and_then(|v| v.as_str());

            if entry_code == code {
                // Check system if provided
                if let Some(expected_system) = system
                    && entry_system != Some(expected_system)
                {
                    continue; // System doesn't match, keep looking
                }

                // Check if abstract (in expansion, abstract is a boolean property)
                let is_abstract = entry
                    .get("abstract")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !abstract_allowed && is_abstract {
                    return Ok(ValidateCodeResult {
                        result: false,
                        message: Some(format!(
                            "Code '{}' is abstract and cannot be used for data entry. Set 'abstractAllowed=true' to allow abstract codes.",
                            code
                        )),
                        display: entry_display.map(String::from),
                    });
                }

                // Found matching code (and system if provided)
                if let Some(expected_display) = display
                    && let Some(actual_display) = entry_display
                    && actual_display != expected_display
                {
                    return Ok(ValidateCodeResult {
                        result: true,
                        message: Some(format!(
                            "Code is valid, but display '{}' does not match expected '{}'",
                            actual_display, expected_display
                        )),
                        display: Some(actual_display.to_string()),
                    });
                }

                return Ok(ValidateCodeResult::valid(entry_display.map(String::from)));
            }
        }

        Ok(ValidateCodeResult::invalid(format!(
            "Code '{}' not found in ValueSet expansion",
            code
        )))
    }

    /// Validate code against ValueSet compose definition.
    async fn validate_in_compose(
        &self,
        compose: &Value,
        code: &str,
        system: Option<&str>,
        display: Option<&str>,
        abstract_allowed: bool,
    ) -> Result<ValidateCodeResult, OperationError> {
        // First check excludes
        if let Some(excludes) = compose.get("exclude").and_then(|v| v.as_array()) {
            for exclude in excludes {
                let exclude_system = exclude.get("system").and_then(|v| v.as_str());
                if let Some(concepts) = exclude.get("concept").and_then(|v| v.as_array()) {
                    for concept in concepts {
                        let concept_code = concept.get("code").and_then(|v| v.as_str());
                        if concept_code == Some(code) {
                            // If system specified, check it matches
                            if let Some(expected_system) = system {
                                if exclude_system == Some(expected_system) {
                                    return Ok(ValidateCodeResult::invalid(format!(
                                        "Code '{}' is explicitly excluded from this ValueSet",
                                        code
                                    )));
                                }
                            } else if exclude_system.is_some() {
                                // No system provided, but code is excluded from some system
                                return Ok(ValidateCodeResult::invalid(format!(
                                    "Code '{}' is explicitly excluded from this ValueSet",
                                    code
                                )));
                            }
                        }
                    }
                }
            }
        }

        // Check includes
        if let Some(includes) = compose.get("include").and_then(|v| v.as_array()) {
            for include in includes {
                let include_system = include.get("system").and_then(|v| v.as_str());

                // If system is specified in params, it must match include's system
                if let Some(expected_system) = system
                    && include_system != Some(expected_system)
                {
                    continue;
                }

                // Check explicit concept list
                if let Some(concepts) = include.get("concept").and_then(|v| v.as_array()) {
                    for concept in concepts {
                        let concept_code = concept.get("code").and_then(|v| v.as_str());
                        let concept_display = concept.get("display").and_then(|v| v.as_str());

                        if concept_code == Some(code) {
                            // Check if abstract
                            if !abstract_allowed && Self::is_abstract_concept(concept) {
                                return Ok(ValidateCodeResult {
                                    result: false,
                                    message: Some(format!(
                                        "Code '{}' is abstract and cannot be used for data entry. Set 'abstractAllowed=true' to allow abstract codes.",
                                        code
                                    )),
                                    display: concept_display.map(String::from),
                                });
                            }

                            // Found the code
                            if let Some(expected_display) = display
                                && let Some(actual_display) = concept_display
                                && actual_display != expected_display
                            {
                                return Ok(ValidateCodeResult {
                                    result: true,
                                    message: Some(format!(
                                        "Code is valid, but display '{}' does not match expected '{}'",
                                        actual_display, expected_display
                                    )),
                                    display: Some(actual_display.to_string()),
                                });
                            }
                            return Ok(ValidateCodeResult::valid(
                                concept_display.map(String::from),
                            ));
                        }
                    }
                } else if include_system.is_some() {
                    // No concept list means all codes from system are included
                    // We need to validate against the CodeSystem
                    if let Some(sys) = include_system {
                        match self.load_code_system_by_url(sys).await {
                            Ok(cs) => {
                                let concepts = cs
                                    .get("concept")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();

                                if let Some(found) = self.find_concept_in_hierarchy(&concepts, code)
                                {
                                    let concept_display =
                                        found.get("display").and_then(|v| v.as_str());

                                    // Check if abstract
                                    if !abstract_allowed && Self::is_abstract_concept(found) {
                                        return Ok(ValidateCodeResult {
                                            result: false,
                                            message: Some(format!(
                                                "Code '{}' is abstract and cannot be used for data entry. Set 'abstractAllowed=true' to allow abstract codes.",
                                                code
                                            )),
                                            display: concept_display.map(String::from),
                                        });
                                    }

                                    if let Some(expected_display) = display
                                        && let Some(actual_display) = concept_display
                                        && actual_display != expected_display
                                    {
                                        return Ok(ValidateCodeResult {
                                            result: true,
                                            message: Some(format!(
                                                "Code is valid, but display '{}' does not match expected '{}'",
                                                actual_display, expected_display
                                            )),
                                            display: Some(actual_display.to_string()),
                                        });
                                    }
                                    return Ok(ValidateCodeResult::valid(
                                        concept_display.map(String::from),
                                    ));
                                }
                            }
                            Err(e) => {
                                tracing::warn!(system = %sys, error = %e, "Failed to load CodeSystem for validation");
                            }
                        }
                    }
                }
            }
        }

        Ok(ValidateCodeResult::invalid(format!(
            "Code '{}' not found in ValueSet",
            code
        )))
    }

    /// Infer the system from a ValueSet when only one system is included.
    fn infer_system_from_valueset(&self, value_set: &Value) -> Option<String> {
        let mut systems: Vec<String> = Vec::new();

        // Check expansion first
        if let Some(expansion) = value_set.get("expansion") {
            if let Some(contains) = expansion.get("contains").and_then(|v| v.as_array()) {
                for entry in contains {
                    if let Some(system) = entry.get("system").and_then(|v| v.as_str()) {
                        if !systems.contains(&system.to_string()) {
                            systems.push(system.to_string());
                        }
                    }
                }
            }
        }

        // Check compose if no expansion
        if systems.is_empty() {
            if let Some(compose) = value_set.get("compose") {
                if let Some(includes) = compose.get("include").and_then(|v| v.as_array()) {
                    for include in includes {
                        if let Some(system) = include.get("system").and_then(|v| v.as_str()) {
                            if !systems.contains(&system.to_string()) {
                                systems.push(system.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Only return if there's exactly one system
        if systems.len() == 1 {
            tracing::debug!(system = %systems[0], "Inferred system from ValueSet");
            Some(systems.remove(0))
        } else {
            tracing::debug!(
                system_count = systems.len(),
                "Cannot infer system: ValueSet contains {} systems",
                systems.len()
            );
            None
        }
    }

    /// Check if a concept is abstract (cannot be selected for data entry).
    fn is_abstract_concept(concept: &Value) -> bool {
        // Check 'abstract' property on the concept
        if let Some(is_abstract) = concept.get("abstract").and_then(|v| v.as_bool()) {
            return is_abstract;
        }

        // Check 'property' array for 'inactive' or 'notSelectable'
        if let Some(properties) = concept.get("property").and_then(|v| v.as_array()) {
            for prop in properties {
                let code = prop.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let value = prop
                    .get("valueBoolean")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if (code == "notSelectable" || code == "abstract") && value {
                    return true;
                }
            }
        }

        false
    }

    /// Find a concept in a hierarchical concept list with depth limiting.
    fn find_concept_in_hierarchy<'a>(
        &self,
        concepts: &'a [Value],
        code: &str,
    ) -> Option<&'a Value> {
        self.find_concept_in_hierarchy_with_depth(concepts, code, 0)
    }

    /// Internal recursive search with depth tracking.
    fn find_concept_in_hierarchy_with_depth<'a>(
        &self,
        concepts: &'a [Value],
        code: &str,
        depth: usize,
    ) -> Option<&'a Value> {
        // Guard against excessive recursion depth
        if depth > MAX_RECURSION_DEPTH {
            tracing::warn!(
                depth = depth,
                code = %code,
                "Maximum recursion depth exceeded in concept hierarchy search"
            );
            return None;
        }

        for concept in concepts {
            let concept_code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");

            if concept_code == code {
                return Some(concept);
            }

            // Check nested concepts
            if let Some(children) = concept.get("concept").and_then(|v| v.as_array())
                && let Some(found) =
                    self.find_concept_in_hierarchy_with_depth(children, code, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }

    /// Load a CodeSystem by URL from canonical manager.
    async fn load_code_system_by_url(&self, url: &str) -> Result<Value, OperationError> {
        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

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

        search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content)
            .ok_or_else(|| {
                OperationError::NotFound(format!("CodeSystem with url '{}' not found", url))
            })
    }

    /// Load a CodeSystem by ID from storage.
    async fn load_code_system_by_id(
        &self,
        state: &AppState,
        id: &str,
    ) -> Result<Value, OperationError> {
        let result =
            state.storage.read("CodeSystem", id).await.map_err(|e| {
                OperationError::Internal(format!("Failed to read CodeSystem: {}", e))
            })?;

        match result {
            Some(stored) => Ok(stored.resource),
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

                    search_result
                        .resources
                        .into_iter()
                        .find(|r| r.resource.content.get("id").and_then(|v| v.as_str()) == Some(id))
                        .map(|r| r.resource.content)
                        .ok_or_else(|| {
                            OperationError::NotFound(format!("CodeSystem '{}' not found", id))
                        })
                } else {
                    Err(OperationError::NotFound(format!(
                        "CodeSystem '{}' not found",
                        id
                    )))
                }
            }
        }
    }

    /// Load a ValueSet by URL from canonical manager.
    async fn load_value_set_by_url(&self, url: &str) -> Result<Value, OperationError> {
        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("ValueSet")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .map_err(|e| {
                OperationError::Internal(format!("Failed to search for ValueSet: {}", e))
            })?;

        search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content)
            .ok_or_else(|| {
                OperationError::NotFound(format!("ValueSet with url '{}' not found", url))
            })
    }

    /// Load a ValueSet by ID from storage.
    async fn load_value_set_by_id(
        &self,
        state: &AppState,
        id: &str,
    ) -> Result<Value, OperationError> {
        let result = state
            .storage
            .read("ValueSet", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to read ValueSet: {}", e)))?;

        match result {
            Some(stored) => Ok(stored.resource),
            None => {
                // Try canonical manager as fallback
                let manager = get_manager();
                if let Some(mgr) = manager {
                    let search_result = mgr
                        .search()
                        .await
                        .resource_type("ValueSet")
                        .text(id)
                        .limit(20)
                        .execute()
                        .await
                        .map_err(|e| {
                            OperationError::Internal(format!("Failed to search canonical: {}", e))
                        })?;

                    search_result
                        .resources
                        .into_iter()
                        .find(|r| r.resource.content.get("id").and_then(|v| v.as_str()) == Some(id))
                        .map(|r| r.resource.content)
                        .ok_or_else(|| {
                            OperationError::NotFound(format!("ValueSet '{}' not found", id))
                        })
                } else {
                    Err(OperationError::NotFound(format!(
                        "ValueSet '{}' not found",
                        id
                    )))
                }
            }
        }
    }

    /// Extract code/system/display from a Coding value.
    fn extract_from_coding(coding: &Value) -> (Option<String>, Option<String>, Option<String>) {
        let code = coding
            .get("code")
            .and_then(|v| v.as_str())
            .map(String::from);
        let system = coding
            .get("system")
            .and_then(|v| v.as_str())
            .map(String::from);
        let display = coding
            .get("display")
            .and_then(|v| v.as_str())
            .map(String::from);
        (code, system, display)
    }

    /// Extract code/system/display from a CodeableConcept value.
    /// Returns the first coding that has a code.
    fn extract_from_codeable_concept(
        cc: &Value,
    ) -> (Option<String>, Option<String>, Option<String>) {
        if let Some(codings) = cc.get("coding").and_then(|v| v.as_array()) {
            for coding in codings {
                let (code, system, display) = Self::extract_from_coding(coding);
                if code.is_some() {
                    return (code, system, display);
                }
            }
        }
        (None, None, None)
    }
}

impl Default for ValidateCodeOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for ValidateCodeOperation {
    fn code(&self) -> &str {
        "validate-code"
    }

    /// Handle system-level $validate-code (POST /$validate-code).
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let validate_params = Self::extract_params(params)?;

        // Extract code/system/display from params (with fallback to coding/codeableConcept)
        let (code, system, display) = if let Some(ref coding) = validate_params.coding {
            Self::extract_from_coding(coding)
        } else if let Some(ref cc) = validate_params.codeable_concept {
            Self::extract_from_codeable_concept(cc)
        } else {
            (
                validate_params.code.clone(),
                validate_params.system.clone(),
                validate_params.display.clone(),
            )
        };

        let code = code.ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'code' parameter (or 'coding' or 'codeableConcept')".into(),
            )
        })?;

        // Determine if this is CodeSystem or ValueSet validation based on URL
        let url = validate_params.url.as_deref();

        if url.is_none() {
            return Err(OperationError::InvalidParameters(
                "The 'url' parameter is required for system-level $validate-code".into(),
            ));
        }

        let infer_system = validate_params.infer_system.unwrap_or(false);
        let abstract_allowed = validate_params.abstract_allowed.unwrap_or(false);

        // Try to determine if it's a CodeSystem or ValueSet URL
        // For now, if system is provided or inferSystem is true, assume ValueSet validation
        // Otherwise try CodeSystem first
        if system.is_some() || infer_system {
            // ValueSet validation
            self.validate_value_set(
                state,
                url,
                None,
                &code,
                system.as_deref(),
                display.as_deref(),
                infer_system,
                abstract_allowed,
            )
            .await
            .map(|r| r.to_parameters())
        } else {
            // Try CodeSystem validation
            self.validate_code_system(
                state,
                url,
                None,
                &code,
                display.as_deref(),
                abstract_allowed,
            )
            .await
            .map(|r| r.to_parameters())
        }
    }

    /// Handle type-level $validate-code (GET/POST /CodeSystem/$validate-code or /ValueSet/$validate-code).
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let validate_params = Self::extract_params(params)?;

        // Extract code/system/display
        let (code, system, display) = if let Some(ref coding) = validate_params.coding {
            Self::extract_from_coding(coding)
        } else if let Some(ref cc) = validate_params.codeable_concept {
            Self::extract_from_codeable_concept(cc)
        } else {
            (
                validate_params.code.clone(),
                validate_params.system.clone(),
                validate_params.display.clone(),
            )
        };

        let code = code.ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'code' parameter (or 'coding' or 'codeableConcept')".into(),
            )
        })?;

        let url = validate_params.url.as_deref();
        let infer_system = validate_params.infer_system.unwrap_or(false);
        let abstract_allowed = validate_params.abstract_allowed.unwrap_or(false);

        match resource_type {
            "CodeSystem" => {
                if url.is_none() {
                    return Err(OperationError::InvalidParameters(
                        "The 'url' parameter is required for type-level CodeSystem/$validate-code"
                            .into(),
                    ));
                }
                self.validate_code_system(
                    state,
                    url,
                    None,
                    &code,
                    display.as_deref(),
                    abstract_allowed,
                )
                .await
                .map(|r| r.to_parameters())
            }
            "ValueSet" => {
                if url.is_none() {
                    return Err(OperationError::InvalidParameters(
                        "The 'url' parameter is required for type-level ValueSet/$validate-code"
                            .into(),
                    ));
                }
                self.validate_value_set(
                    state,
                    url,
                    None,
                    &code,
                    system.as_deref(),
                    display.as_deref(),
                    infer_system,
                    abstract_allowed,
                )
                .await
                .map(|r| r.to_parameters())
            }
            _ => Err(OperationError::NotSupported(format!(
                "$validate-code is only supported on CodeSystem and ValueSet, not {}",
                resource_type
            ))),
        }
    }

    /// Handle instance-level $validate-code (GET/POST /CodeSystem/{id}/$validate-code or /ValueSet/{id}/$validate-code).
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let validate_params = Self::extract_params(params)?;

        // Extract code/system/display
        let (code, system, display) = if let Some(ref coding) = validate_params.coding {
            Self::extract_from_coding(coding)
        } else if let Some(ref cc) = validate_params.codeable_concept {
            Self::extract_from_codeable_concept(cc)
        } else {
            (
                validate_params.code.clone(),
                validate_params.system.clone(),
                validate_params.display.clone(),
            )
        };

        let code = code.ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'code' parameter (or 'coding' or 'codeableConcept')".into(),
            )
        })?;

        let infer_system = validate_params.infer_system.unwrap_or(false);
        let abstract_allowed = validate_params.abstract_allowed.unwrap_or(false);

        match resource_type {
            "CodeSystem" => self
                .validate_code_system(
                    state,
                    None,
                    Some(id),
                    &code,
                    display.as_deref(),
                    abstract_allowed,
                )
                .await
                .map(|r| r.to_parameters()),
            "ValueSet" => self
                .validate_value_set(
                    state,
                    None,
                    Some(id),
                    &code,
                    system.as_deref(),
                    display.as_deref(),
                    infer_system,
                    abstract_allowed,
                )
                .await
                .map(|r| r.to_parameters()),
            _ => Err(OperationError::NotSupported(format!(
                "$validate-code is only supported on CodeSystem and ValueSet, not {}",
                resource_type
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_params_from_query() {
        let params = json!({
            "url": "http://example.org/fhir/CodeSystem/test",
            "code": "ABC",
            "system": "http://example.org/codes",
            "display": "ABC Code"
        });

        let validate_params = ValidateCodeOperation::extract_params(&params).unwrap();
        assert_eq!(
            validate_params.url,
            Some("http://example.org/fhir/CodeSystem/test".to_string())
        );
        assert_eq!(validate_params.code, Some("ABC".to_string()));
        assert_eq!(
            validate_params.system,
            Some("http://example.org/codes".to_string())
        );
        assert_eq!(validate_params.display, Some("ABC Code".to_string()));
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "url", "valueUri": "http://example.org/fhir/ValueSet/test"},
                {"name": "code", "valueCode": "XYZ"},
                {"name": "system", "valueUri": "http://example.org/codes"}
            ]
        });

        let validate_params = ValidateCodeOperation::extract_params(&params).unwrap();
        assert_eq!(
            validate_params.url,
            Some("http://example.org/fhir/ValueSet/test".to_string())
        );
        assert_eq!(validate_params.code, Some("XYZ".to_string()));
        assert_eq!(
            validate_params.system,
            Some("http://example.org/codes".to_string())
        );
    }

    #[test]
    fn test_validate_code_result_to_parameters() {
        let result = ValidateCodeResult {
            result: true,
            message: None,
            display: Some("Test Display".to_string()),
        };

        let params = result.to_parameters();
        assert_eq!(params["resourceType"], "Parameters");

        let parameters = params["parameter"].as_array().unwrap();
        assert!(
            parameters
                .iter()
                .any(|p| { p["name"] == "result" && p["valueBoolean"] == true })
        );
        assert!(
            parameters
                .iter()
                .any(|p| { p["name"] == "display" && p["valueString"] == "Test Display" })
        );
    }

    #[test]
    fn test_extract_from_coding() {
        let coding = json!({
            "system": "http://example.org",
            "code": "ABC",
            "display": "ABC Display"
        });

        let (code, system, display) = ValidateCodeOperation::extract_from_coding(&coding);
        assert_eq!(code, Some("ABC".to_string()));
        assert_eq!(system, Some("http://example.org".to_string()));
        assert_eq!(display, Some("ABC Display".to_string()));
    }

    #[test]
    fn test_extract_from_codeable_concept() {
        let cc = json!({
            "coding": [
                {
                    "system": "http://example.org",
                    "code": "XYZ",
                    "display": "XYZ Display"
                }
            ],
            "text": "XYZ Text"
        });

        let (code, system, display) = ValidateCodeOperation::extract_from_codeable_concept(&cc);
        assert_eq!(code, Some("XYZ".to_string()));
        assert_eq!(system, Some("http://example.org".to_string()));
        assert_eq!(display, Some("XYZ Display".to_string()));
    }

    #[test]
    fn test_find_concept_in_hierarchy() {
        let op = ValidateCodeOperation::new();
        let concepts: Vec<Value> = vec![
            json!({
                "code": "A",
                "display": "Code A",
                "concept": [
                    {
                        "code": "A1",
                        "display": "Code A1"
                    },
                    {
                        "code": "A2",
                        "display": "Code A2"
                    }
                ]
            }),
            json!({
                "code": "B",
                "display": "Code B"
            }),
        ];

        // Find top-level code
        let found = op.find_concept_in_hierarchy(&concepts, "A");
        assert!(found.is_some());
        assert_eq!(found.unwrap()["display"], "Code A");

        // Find nested code
        let found = op.find_concept_in_hierarchy(&concepts, "A1");
        assert!(found.is_some());
        assert_eq!(found.unwrap()["display"], "Code A1");

        // Not found
        let found = op.find_concept_in_hierarchy(&concepts, "Z");
        assert!(found.is_none());
    }

    #[test]
    fn test_infer_system_from_valueset_single_system() {
        let op = ValidateCodeOperation::new();

        // ValueSet with single system in compose
        let valueset = json!({
            "resourceType": "ValueSet",
            "compose": {
                "include": [{
                    "system": "http://example.org/codes"
                }]
            }
        });

        let system = op.infer_system_from_valueset(&valueset);
        assert_eq!(system, Some("http://example.org/codes".to_string()));
    }

    #[test]
    fn test_infer_system_from_valueset_multiple_systems() {
        let op = ValidateCodeOperation::new();

        // ValueSet with multiple systems - cannot infer
        let valueset = json!({
            "resourceType": "ValueSet",
            "compose": {
                "include": [
                    {"system": "http://example.org/codes1"},
                    {"system": "http://example.org/codes2"}
                ]
            }
        });

        let system = op.infer_system_from_valueset(&valueset);
        assert!(system.is_none());
    }

    #[test]
    fn test_infer_system_from_valueset_expansion() {
        let op = ValidateCodeOperation::new();

        // ValueSet with expansion
        let valueset = json!({
            "resourceType": "ValueSet",
            "expansion": {
                "contains": [
                    {"system": "http://example.org/codes", "code": "A"},
                    {"system": "http://example.org/codes", "code": "B"}
                ]
            }
        });

        let system = op.infer_system_from_valueset(&valueset);
        assert_eq!(system, Some("http://example.org/codes".to_string()));
    }

    #[test]
    fn test_is_abstract_concept_boolean_property() {
        // Abstract concept with boolean property
        let concept = json!({
            "code": "A",
            "display": "Abstract Code A",
            "abstract": true
        });

        assert!(ValidateCodeOperation::is_abstract_concept(&concept));

        // Non-abstract concept
        let concept = json!({
            "code": "B",
            "display": "Concrete Code B",
            "abstract": false
        });

        assert!(!ValidateCodeOperation::is_abstract_concept(&concept));
    }

    #[test]
    fn test_is_abstract_concept_property_array() {
        // Abstract concept with property array
        let concept = json!({
            "code": "A",
            "display": "Abstract Code A",
            "property": [{
                "code": "notSelectable",
                "valueBoolean": true
            }]
        });

        assert!(ValidateCodeOperation::is_abstract_concept(&concept));

        // Non-abstract with property array
        let concept = json!({
            "code": "B",
            "display": "Concrete Code B",
            "property": [{
                "code": "notSelectable",
                "valueBoolean": false
            }]
        });

        assert!(!ValidateCodeOperation::is_abstract_concept(&concept));
    }

    #[test]
    fn test_extract_params_with_infer_system() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "url", "valueUri": "http://example.org/fhir/ValueSet/test"},
                {"name": "code", "valueCode": "ABC"},
                {"name": "inferSystem", "valueBoolean": true},
                {"name": "abstractAllowed", "valueBoolean": true}
            ]
        });

        let validate_params = ValidateCodeOperation::extract_params(&params).unwrap();
        assert_eq!(validate_params.infer_system, Some(true));
        assert_eq!(validate_params.abstract_allowed, Some(true));
    }
}
