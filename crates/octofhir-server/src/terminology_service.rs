//! Server Terminology Service
//!
//! Implements the TerminologyService trait from octofhir-fhirschema to enable
//! binding validation during resource validation. This service uses the
//! canonical manager to resolve ValueSets and CodeSystems.
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_server::terminology_service::ServerTerminologyService;
//!
//! let service = Arc::new(ServerTerminologyService::new());
//!
//! // Use with fhirschema validator
//! let validator = FhirSchemaValidator::new(schemas, fhirpath)
//!     .with_terminology_service(service);
//! ```

use async_trait::async_trait;
use octofhir_fhirschema::terminology::{
    CodeValidationResult, TerminologyError, TerminologyResult, TerminologyService,
};
use serde_json::Value;

use crate::canonical::get_manager;

/// Server-side terminology service for binding validation.
///
/// Uses the canonical manager to resolve ValueSets and CodeSystems
/// and validate codes against them.
#[derive(Debug, Clone, Default)]
pub struct ServerTerminologyService;

impl ServerTerminologyService {
    /// Create a new server terminology service
    pub fn new() -> Self {
        Self
    }

    /// Load a ValueSet by URL from the canonical manager
    async fn load_value_set(&self, url: &str) -> Option<Value> {
        let manager = get_manager()?;

        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("ValueSet")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .ok()?;

        search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content)
    }

    /// Load a CodeSystem by URL from the canonical manager
    async fn load_code_system(&self, url: &str) -> Option<Value> {
        let manager = get_manager()?;

        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("CodeSystem")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .ok()?;

        search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content)
    }

    /// Find a concept in a CodeSystem's hierarchical concept list
    fn find_concept_in_hierarchy<'a>(&self, concepts: &'a [Value], code: &str) -> Option<&'a Value> {
        for concept in concepts {
            let concept_code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");

            if concept_code == code {
                return Some(concept);
            }

            // Check nested concepts
            if let Some(children) = concept.get("concept").and_then(|v| v.as_array())
                && let Some(found) = self.find_concept_in_hierarchy(children, code) {
                    return Some(found);
                }
        }
        None
    }

    /// Validate a code against a ValueSet's expansion
    fn validate_in_expansion(
        &self,
        expansion: &Value,
        code: &str,
        system: Option<&str>,
    ) -> Option<CodeValidationResult> {
        let contains = expansion
            .get("contains")
            .and_then(|v| v.as_array())?;

        for entry in contains {
            let entry_code = entry.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let entry_system = entry.get("system").and_then(|v| v.as_str());
            let entry_display = entry.get("display").and_then(|v| v.as_str());

            if entry_code == code {
                // Check system if provided
                if let Some(expected_system) = system
                    && entry_system != Some(expected_system) {
                        continue; // System doesn't match, keep looking
                    }

                // Found matching code
                return Some(CodeValidationResult::valid_with_display(
                    entry_display.unwrap_or(code).to_string(),
                ));
            }
        }

        None
    }

    /// Validate a code against a ValueSet's compose definition
    async fn validate_in_compose(
        &self,
        compose: &Value,
        code: &str,
        system: Option<&str>,
    ) -> Option<CodeValidationResult> {
        // First check excludes
        if let Some(excludes) = compose.get("exclude").and_then(|v| v.as_array()) {
            for exclude in excludes {
                let exclude_system = exclude.get("system").and_then(|v| v.as_str());
                if let Some(concepts) = exclude.get("concept").and_then(|v| v.as_array()) {
                    for concept in concepts {
                        let concept_code = concept.get("code").and_then(|v| v.as_str());
                        if concept_code == Some(code) {
                            // Check if system matches
                            if system.is_none() || exclude_system == system {
                                return Some(CodeValidationResult::invalid());
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

                // If system is specified, it must match
                if let Some(expected_system) = system
                    && include_system != Some(expected_system) {
                        continue;
                    }

                // Check explicit concept list
                if let Some(concepts) = include.get("concept").and_then(|v| v.as_array()) {
                    for concept in concepts {
                        let concept_code = concept.get("code").and_then(|v| v.as_str());
                        let concept_display = concept.get("display").and_then(|v| v.as_str());

                        if concept_code == Some(code) {
                            return Some(CodeValidationResult::valid_with_display(
                                concept_display.unwrap_or(code).to_string(),
                            ));
                        }
                    }
                } else if let Some(sys) = include_system {
                    // No concept list means all codes from system are included
                    // Validate against the CodeSystem
                    if let Some(cs) = self.load_code_system(sys).await {
                        let concepts = cs
                            .get("concept")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        if let Some(found) = self.find_concept_in_hierarchy(&concepts, code) {
                            let display = found
                                .get("display")
                                .and_then(|v| v.as_str())
                                .unwrap_or(code);
                            return Some(CodeValidationResult::valid_with_display(
                                display.to_string(),
                            ));
                        }
                    }
                }
            }
        }

        None
    }
}

#[async_trait]
impl TerminologyService for ServerTerminologyService {
    /// Validate a code against a value set.
    ///
    /// This is used during resource validation to check that coded elements
    /// contain codes that are valid for their bound value sets.
    async fn validate_code(
        &self,
        value_set_url: &str,
        code: &str,
        system: Option<&str>,
    ) -> TerminologyResult<CodeValidationResult> {
        // Load the ValueSet
        let value_set = self.load_value_set(value_set_url).await.ok_or_else(|| {
            TerminologyError::ValueSetNotFound {
                url: value_set_url.to_string(),
            }
        })?;

        // First check if there's an expansion
        if let Some(expansion) = value_set.get("expansion") {
            if let Some(result) = self.validate_in_expansion(expansion, code, system) {
                return Ok(result);
            }
            // Code not found in expansion
            return Ok(CodeValidationResult::invalid());
        }

        // Check compose
        if let Some(compose) = value_set.get("compose")
            && let Some(result) = self.validate_in_compose(compose, code, system).await {
                return Ok(result);
            }

        // Code not found
        Ok(CodeValidationResult::invalid())
    }

    /// Check if a value set exists.
    async fn value_set_exists(&self, value_set_url: &str) -> TerminologyResult<bool> {
        Ok(self.load_value_set(value_set_url).await.is_some())
    }

    /// Get the display text for a code from a code system.
    async fn get_display(&self, system: &str, code: &str) -> TerminologyResult<Option<String>> {
        let code_system = match self.load_code_system(system).await {
            Some(cs) => cs,
            None => return Ok(None),
        };

        let concepts = code_system
            .get("concept")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if let Some(found) = self.find_concept_in_hierarchy(&concepts, code) {
            return Ok(found.get("display").and_then(|v| v.as_str()).map(String::from));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_find_concept_in_hierarchy() {
        let service = ServerTerminologyService::new();
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
        let found = service.find_concept_in_hierarchy(&concepts, "A");
        assert!(found.is_some());
        assert_eq!(found.unwrap()["display"], "Code A");

        // Find nested code
        let found = service.find_concept_in_hierarchy(&concepts, "A1");
        assert!(found.is_some());
        assert_eq!(found.unwrap()["display"], "Code A1");

        // Not found
        let found = service.find_concept_in_hierarchy(&concepts, "Z");
        assert!(found.is_none());
    }

    #[test]
    fn test_validate_in_expansion() {
        let service = ServerTerminologyService::new();
        let expansion = json!({
            "contains": [
                {
                    "system": "http://example.org/codes",
                    "code": "A",
                    "display": "Code A"
                },
                {
                    "system": "http://example.org/codes",
                    "code": "B",
                    "display": "Code B"
                }
            ]
        });

        // Valid code
        let result = service.validate_in_expansion(&expansion, "A", None);
        assert!(result.is_some());
        assert!(result.as_ref().unwrap().valid);
        assert_eq!(result.unwrap().display, Some("Code A".to_string()));

        // Valid code with system check
        let result = service.validate_in_expansion(&expansion, "A", Some("http://example.org/codes"));
        assert!(result.is_some());
        assert!(result.unwrap().valid);

        // Invalid code
        let result = service.validate_in_expansion(&expansion, "Z", None);
        assert!(result.is_none());

        // Wrong system
        let result = service.validate_in_expansion(&expansion, "A", Some("http://other.org"));
        assert!(result.is_none());
    }
}
