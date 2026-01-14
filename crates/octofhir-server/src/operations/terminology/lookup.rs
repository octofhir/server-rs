//! CodeSystem $lookup Operation
//!
//! Implements the FHIR CodeSystem/$lookup operation to look up a code
//! within a code system and return information about it.
//!
//! Specification: http://hl7.org/fhir/codesystem-operation-lookup.html
//!
//! Supported invocation levels:
//! - System: `POST /$lookup` with system and code parameters
//! - Type: `GET/POST /CodeSystem/$lookup` with system/url and code parameters
//! - Instance: `GET/POST /CodeSystem/{id}/$lookup` with code parameter

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

/// Parameters for the $lookup operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupParams {
    /// The code to look up
    pub code: Option<String>,

    /// The code system URL (system parameter)
    pub system: Option<String>,

    /// A Coding to look up (alternative to code+system)
    #[serde(default)]
    pub coding: Option<Value>,

    /// The version of the code system
    pub version: Option<String>,

    /// The date of the code system to use (for temporal lookups)
    pub date: Option<String>,

    /// Properties to return (if not specified, return all available)
    #[serde(default)]
    pub property: Vec<String>,

    /// Language to use for display and designations
    pub display_language: Option<String>,
}

/// A designation (translation/synonym) for a concept.
#[derive(Debug, Clone)]
pub struct Designation {
    pub language: Option<String>,
    pub use_coding: Option<Value>,
    pub value: String,
}

impl Designation {
    fn to_parameters_part(&self) -> Value {
        let mut parts = vec![json!({
            "name": "value",
            "valueString": self.value
        })];

        if let Some(ref lang) = self.language {
            parts.push(json!({
                "name": "language",
                "valueCode": lang
            }));
        }

        if let Some(ref use_coding) = self.use_coding {
            parts.push(json!({
                "name": "use",
                "valueCoding": use_coding
            }));
        }

        json!({
            "name": "designation",
            "part": parts
        })
    }
}

/// A property of a concept.
#[derive(Debug, Clone)]
pub struct ConceptProperty {
    pub code: String,
    pub value: PropertyValue,
    pub description: Option<String>,
}

/// Property value types.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    Code(String),
    Coding(Value),
    String(String),
    Integer(i64),
    Boolean(bool),
    DateTime(String),
    Decimal(f64),
}

impl ConceptProperty {
    fn to_parameters_part(&self) -> Value {
        let mut parts = vec![json!({
            "name": "code",
            "valueCode": self.code
        })];

        // Add the value with appropriate type
        match &self.value {
            PropertyValue::Code(v) => parts.push(json!({
                "name": "value",
                "valueCode": v
            })),
            PropertyValue::Coding(v) => parts.push(json!({
                "name": "value",
                "valueCoding": v
            })),
            PropertyValue::String(v) => parts.push(json!({
                "name": "value",
                "valueString": v
            })),
            PropertyValue::Integer(v) => parts.push(json!({
                "name": "value",
                "valueInteger": v
            })),
            PropertyValue::Boolean(v) => parts.push(json!({
                "name": "value",
                "valueBoolean": v
            })),
            PropertyValue::DateTime(v) => parts.push(json!({
                "name": "value",
                "valueDateTime": v
            })),
            PropertyValue::Decimal(v) => parts.push(json!({
                "name": "value",
                "valueDecimal": v
            })),
        }

        if let Some(ref desc) = self.description {
            parts.push(json!({
                "name": "description",
                "valueString": desc
            }));
        }

        json!({
            "name": "property",
            "part": parts
        })
    }
}

/// Result of a code lookup.
#[derive(Debug)]
pub struct LookupResult {
    /// Name of the code system
    pub name: String,
    /// Version of the code system
    pub version: Option<String>,
    /// Display text for the code
    pub display: Option<String>,
    /// Definition of the code
    pub definition: Option<String>,
    /// Designations (translations, synonyms)
    pub designations: Vec<Designation>,
    /// Properties of the concept
    pub properties: Vec<ConceptProperty>,
}

impl LookupResult {
    /// Convert to FHIR Parameters resource.
    pub fn to_parameters(&self) -> Value {
        let mut params = vec![json!({
            "name": "name",
            "valueString": self.name
        })];

        if let Some(ref version) = self.version {
            params.push(json!({
                "name": "version",
                "valueString": version
            }));
        }

        if let Some(ref display) = self.display {
            params.push(json!({
                "name": "display",
                "valueString": display
            }));
        }

        if let Some(ref definition) = self.definition {
            params.push(json!({
                "name": "definition",
                "valueString": definition
            }));
        }

        // Add designations
        for designation in &self.designations {
            params.push(designation.to_parameters_part());
        }

        // Add properties
        for property in &self.properties {
            params.push(property.to_parameters_part());
        }

        json!({
            "resourceType": "Parameters",
            "parameter": params
        })
    }
}

/// The $lookup operation handler.
pub struct LookupOperation;

impl LookupOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<LookupParams, OperationError> {
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut lookup_params = LookupParams::default();

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "code" => {
                            lookup_params.code = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "system" => {
                            lookup_params.system = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "coding" => {
                            lookup_params.coding = param.get("valueCoding").cloned();
                        }
                        "version" => {
                            lookup_params.version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "date" => {
                            lookup_params.date = param
                                .get("valueDateTime")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "property" => {
                            if let Some(prop) = param.get("valueCode").and_then(|v| v.as_str()) {
                                lookup_params.property.push(prop.to_string());
                            }
                        }
                        "displayLanguage" => {
                            lookup_params.display_language = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        _ => {}
                    }
                }
            }

            Ok(lookup_params)
        } else {
            // Assume it's a flat object with query parameters
            let lookup_params: LookupParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok(lookup_params)
        }
    }

    /// Extract code and system from params, with coding fallback.
    fn resolve_code_system(params: &LookupParams) -> Result<(String, String), OperationError> {
        // Try coding first
        if let Some(ref coding) = params.coding {
            let code = coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from);
            let system = coding
                .get("system")
                .and_then(|v| v.as_str())
                .map(String::from);

            if let (Some(code), Some(system)) = (code, system) {
                return Ok((code, system));
            }
        }

        // Fall back to explicit parameters
        let code = params.code.clone().ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'code' parameter (or 'coding' with code)".into(),
            )
        })?;

        let system = params.system.clone().ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'system' parameter (or 'coding' with system)".into(),
            )
        })?;

        Ok((code, system))
    }

    /// Load a CodeSystem by URL from cache or canonical manager.
    async fn load_code_system_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Value, OperationError> {
        let cache = get_cache();
        let cache_key = CodeSystemKey::new(url, version.map(String::from));

        // Check cache first
        if let Some(cached) = cache.get_code_system(&cache_key).await {
            tracing::debug!(url = %url, "CodeSystem loaded from cache");
            return Ok(cached.as_ref().clone());
        }

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

        // Cache the result (even if None to avoid repeated lookups)
        cache.insert_code_system(cache_key, result.clone()).await;

        result.ok_or_else(|| OperationError::NotFound(format!("CodeSystem '{}' not found", url)))
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

    /// Find a concept in a hierarchical concept list with depth limiting.
    fn find_concept_in_hierarchy<'a>(
        &self,
        concepts: &'a [Value],
        code: &str,
        depth: usize,
    ) -> Option<&'a Value> {
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
                && let Some(found) = self.find_concept_in_hierarchy(children, code, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }

    /// Build lookup result from CodeSystem and found concept.
    fn build_result(
        &self,
        code_system: &Value,
        concept: &Value,
        requested_properties: &[String],
    ) -> LookupResult {
        let name = code_system
            .get("name")
            .or_else(|| code_system.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let version = code_system
            .get("version")
            .and_then(|v| v.as_str())
            .map(String::from);

        let display = concept
            .get("display")
            .and_then(|v| v.as_str())
            .map(String::from);

        let definition = concept
            .get("definition")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Extract designations
        let designations = self.extract_designations(concept);

        // Extract properties
        let properties = self.extract_properties(concept, code_system, requested_properties);

        LookupResult {
            name,
            version,
            display,
            definition,
            designations,
            properties,
        }
    }

    /// Extract designations from a concept.
    fn extract_designations(&self, concept: &Value) -> Vec<Designation> {
        let mut designations = Vec::new();

        if let Some(desigs) = concept.get("designation").and_then(|v| v.as_array()) {
            for desig in desigs {
                let language = desig
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let use_coding = desig.get("use").cloned();
                let value = desig
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if !value.is_empty() {
                    designations.push(Designation {
                        language,
                        use_coding,
                        value,
                    });
                }
            }
        }

        designations
    }

    /// Extract properties from a concept.
    fn extract_properties(
        &self,
        concept: &Value,
        code_system: &Value,
        requested_properties: &[String],
    ) -> Vec<ConceptProperty> {
        let mut properties = Vec::new();

        if let Some(props) = concept.get("property").and_then(|v| v.as_array()) {
            for prop in props {
                let code = prop
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Skip if not in requested properties (unless all are requested)
                if !requested_properties.is_empty() && !requested_properties.contains(&code) {
                    continue;
                }

                // Determine value type and extract
                let value = if let Some(v) = prop.get("valueCode").and_then(|v| v.as_str()) {
                    PropertyValue::Code(v.to_string())
                } else if let Some(v) = prop.get("valueCoding") {
                    PropertyValue::Coding(v.clone())
                } else if let Some(v) = prop.get("valueString").and_then(|v| v.as_str()) {
                    PropertyValue::String(v.to_string())
                } else if let Some(v) = prop.get("valueInteger").and_then(|v| v.as_i64()) {
                    PropertyValue::Integer(v)
                } else if let Some(v) = prop.get("valueBoolean").and_then(|v| v.as_bool()) {
                    PropertyValue::Boolean(v)
                } else if let Some(v) = prop.get("valueDateTime").and_then(|v| v.as_str()) {
                    PropertyValue::DateTime(v.to_string())
                } else if let Some(v) = prop.get("valueDecimal").and_then(|v| v.as_f64()) {
                    PropertyValue::Decimal(v)
                } else {
                    // Try generic value field
                    if let Some(v) = prop.get("value").and_then(|v| v.as_str()) {
                        PropertyValue::String(v.to_string())
                    } else {
                        continue; // No recognizable value
                    }
                };

                // Look up property description from CodeSystem
                let description = self.get_property_description(code_system, &code);

                properties.push(ConceptProperty {
                    code,
                    value,
                    description,
                });
            }
        }

        properties
    }

    /// Get property description from CodeSystem definition.
    fn get_property_description(&self, code_system: &Value, property_code: &str) -> Option<String> {
        if let Some(props) = code_system.get("property").and_then(|v| v.as_array()) {
            for prop in props {
                if prop.get("code").and_then(|v| v.as_str()) == Some(property_code) {
                    return prop
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
        }
        None
    }

    /// Perform the lookup operation.
    async fn lookup(
        &self,
        state: &AppState,
        code_system_url: Option<&str>,
        code_system_id: Option<&str>,
        code: &str,
        version: Option<&str>,
        requested_properties: &[String],
    ) -> Result<LookupResult, OperationError> {
        // Load the CodeSystem
        let code_system = if let Some(id) = code_system_id {
            self.load_code_system_by_id(state, id).await?
        } else if let Some(url) = code_system_url {
            self.load_code_system_by_url(url, version).await?
        } else {
            return Err(OperationError::InvalidParameters(
                "Either 'system' parameter or CodeSystem instance ID is required".into(),
            ));
        };

        // Find the concept in the CodeSystem
        let concepts = code_system
            .get("concept")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let concept = self
            .find_concept_in_hierarchy(&concepts, code, 0)
            .ok_or_else(|| {
                let cs_name = code_system
                    .get("name")
                    .or_else(|| code_system.get("url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                OperationError::NotFound(format!(
                    "Code '{}' not found in CodeSystem '{}'",
                    code, cs_name
                ))
            })?;

        // Build and return the result
        Ok(self.build_result(&code_system, concept, requested_properties))
    }
}

impl Default for LookupOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for LookupOperation {
    fn code(&self) -> &str {
        "lookup"
    }

    /// Handle system-level $lookup (POST /$lookup).
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let lookup_params = Self::extract_params(params)?;
        let (code, system) = Self::resolve_code_system(&lookup_params)?;

        self.lookup(
            state,
            Some(&system),
            None,
            &code,
            lookup_params.version.as_deref(),
            &lookup_params.property,
        )
        .await
        .map(|r| r.to_parameters())
    }

    /// Handle type-level $lookup (GET/POST /CodeSystem/$lookup).
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "CodeSystem" {
            return Err(OperationError::NotSupported(format!(
                "$lookup is only supported on CodeSystem, not {}",
                resource_type
            )));
        }

        let lookup_params = Self::extract_params(params)?;
        let (code, system) = Self::resolve_code_system(&lookup_params)?;

        self.lookup(
            state,
            Some(&system),
            None,
            &code,
            lookup_params.version.as_deref(),
            &lookup_params.property,
        )
        .await
        .map(|r| r.to_parameters())
    }

    /// Handle instance-level $lookup (GET/POST /CodeSystem/{id}/$lookup).
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "CodeSystem" {
            return Err(OperationError::NotSupported(format!(
                "$lookup is only supported on CodeSystem, not {}",
                resource_type
            )));
        }

        let lookup_params = Self::extract_params(params)?;

        // For instance-level, code is still required but system comes from the instance
        let code = if let Some(ref coding) = lookup_params.coding {
            coding
                .get("code")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            lookup_params.code.clone()
        };

        let code = code
            .ok_or_else(|| OperationError::InvalidParameters("Missing 'code' parameter".into()))?;

        self.lookup(
            state,
            None,
            Some(id),
            &code,
            lookup_params.version.as_deref(),
            &lookup_params.property,
        )
        .await
        .map(|r| r.to_parameters())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_params_from_query() {
        let params = json!({
            "code": "8480-6",
            "system": "http://loinc.org",
            "version": "2.73"
        });

        let lookup_params = LookupOperation::extract_params(&params).unwrap();
        assert_eq!(lookup_params.code, Some("8480-6".to_string()));
        assert_eq!(lookup_params.system, Some("http://loinc.org".to_string()));
        assert_eq!(lookup_params.version, Some("2.73".to_string()));
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "code", "valueCode": "73211009"},
                {"name": "system", "valueUri": "http://snomed.info/sct"}
            ]
        });

        let lookup_params = LookupOperation::extract_params(&params).unwrap();
        assert_eq!(lookup_params.code, Some("73211009".to_string()));
        assert_eq!(
            lookup_params.system,
            Some("http://snomed.info/sct".to_string())
        );
    }

    #[test]
    fn test_resolve_code_system_from_params() {
        let params = LookupParams {
            code: Some("test-code".to_string()),
            system: Some("http://example.org".to_string()),
            ..Default::default()
        };

        let (code, system) = LookupOperation::resolve_code_system(&params).unwrap();
        assert_eq!(code, "test-code");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_resolve_code_system_from_coding() {
        let params = LookupParams {
            coding: Some(json!({
                "system": "http://example.org",
                "code": "test-code"
            })),
            ..Default::default()
        };

        let (code, system) = LookupOperation::resolve_code_system(&params).unwrap();
        assert_eq!(code, "test-code");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_lookup_result_to_parameters() {
        let result = LookupResult {
            name: "Test CodeSystem".to_string(),
            version: Some("1.0".to_string()),
            display: Some("Test Display".to_string()),
            definition: Some("Test Definition".to_string()),
            designations: vec![Designation {
                language: Some("en".to_string()),
                use_coding: None,
                value: "English Display".to_string(),
            }],
            properties: vec![ConceptProperty {
                code: "status".to_string(),
                value: PropertyValue::Code("active".to_string()),
                description: Some("Status of the concept".to_string()),
            }],
        };

        let params = result.to_parameters();
        assert_eq!(params["resourceType"], "Parameters");

        let parameters = params["parameter"].as_array().unwrap();
        assert!(
            parameters
                .iter()
                .any(|p| p["name"] == "name" && p["valueString"] == "Test CodeSystem")
        );
        assert!(
            parameters
                .iter()
                .any(|p| p["name"] == "display" && p["valueString"] == "Test Display")
        );
        assert!(parameters.iter().any(|p| p["name"] == "designation"));
        assert!(parameters.iter().any(|p| p["name"] == "property"));
    }
}
