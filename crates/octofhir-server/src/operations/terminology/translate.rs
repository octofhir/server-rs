//! ConceptMap $translate Operation
//!
//! Implements the FHIR ConceptMap/$translate operation to translate a code
//! from one code system to another using ConceptMap resources.
//!
//! Specification: http://hl7.org/fhir/conceptmap-operation-translate.html
//!
//! Supported invocation levels:
//! - System: `POST /$translate` with code and source/target parameters
//! - Type: `GET/POST /ConceptMap/$translate`
//! - Instance: `GET/POST /ConceptMap/{id}/$translate`

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::canonical::get_manager;
use crate::operations::terminology::cache::{ConceptMapKey, get_cache};
use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

/// Parameters for the $translate operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateParams {
    /// The code to translate
    pub code: Option<String>,

    /// The code system of the source code
    pub system: Option<String>,

    /// The version of the source code system
    pub version: Option<String>,

    /// A Coding to translate (alternative to code+system)
    #[serde(default)]
    pub coding: Option<Value>,

    /// A CodeableConcept to translate
    #[serde(default)]
    pub codeable_concept: Option<Value>,

    /// The source ValueSet URL (scope of source codes)
    pub source: Option<String>,

    /// The target ValueSet or CodeSystem URL
    pub target: Option<String>,

    /// Specific ConceptMap URL to use
    #[serde(rename = "conceptMap", alias = "url")]
    pub concept_map_url: Option<String>,

    /// Target code system URL (when target is not a ValueSet)
    pub target_system: Option<String>,

    /// Reverse translation (target -> source)
    #[serde(default)]
    pub reverse: bool,
}

/// A translation match from the ConceptMap.
#[derive(Debug, Clone)]
pub struct TranslationMatch {
    /// The equivalence of this mapping
    pub equivalence: String,
    /// The target concept
    pub concept: TranslatedConcept,
    /// The ConceptMap that provided this translation
    pub source: Option<String>,
    /// Additional properties of the mapping
    pub product: Vec<TranslationProduct>,
}

/// A translated concept.
#[derive(Debug, Clone)]
pub struct TranslatedConcept {
    pub system: Option<String>,
    pub code: Option<String>,
    pub display: Option<String>,
}

/// Additional product of a translation.
#[derive(Debug, Clone)]
pub struct TranslationProduct {
    pub property: String,
    pub system: Option<String>,
    pub code: Option<String>,
    pub display: Option<String>,
}

/// Result of a translation operation.
#[derive(Debug)]
pub struct TranslateResult {
    /// Whether translation was successful
    pub result: bool,
    /// Translation matches
    pub matches: Vec<TranslationMatch>,
    /// Optional message
    pub message: Option<String>,
}

impl TranslateResult {
    /// Create a successful result with matches.
    pub fn success(matches: Vec<TranslationMatch>) -> Self {
        Self {
            result: !matches.is_empty(),
            matches,
            message: None,
        }
    }

    /// Create a failed result with message.
    pub fn failure(message: String) -> Self {
        Self {
            result: false,
            matches: Vec::new(),
            message: Some(message),
        }
    }

    /// Convert to FHIR Parameters resource.
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

        for m in &self.matches {
            let mut match_parts = Vec::new();

            // Equivalence
            match_parts.push(json!({
                "name": "equivalence",
                "valueCode": m.equivalence
            }));

            // Concept (as Coding)
            let mut coding = json!({});
            if let Some(ref system) = m.concept.system {
                coding["system"] = json!(system);
            }
            if let Some(ref code) = m.concept.code {
                coding["code"] = json!(code);
            }
            if let Some(ref display) = m.concept.display {
                coding["display"] = json!(display);
            }
            match_parts.push(json!({
                "name": "concept",
                "valueCoding": coding
            }));

            // Source (ConceptMap URL)
            if let Some(ref source) = m.source {
                match_parts.push(json!({
                    "name": "source",
                    "valueUri": source
                }));
            }

            // Products
            for product in &m.product {
                let mut product_parts = vec![json!({
                    "name": "property",
                    "valueUri": product.property
                })];

                let mut prod_coding = json!({});
                if let Some(ref system) = product.system {
                    prod_coding["system"] = json!(system);
                }
                if let Some(ref code) = product.code {
                    prod_coding["code"] = json!(code);
                }
                if let Some(ref display) = product.display {
                    prod_coding["display"] = json!(display);
                }
                product_parts.push(json!({
                    "name": "concept",
                    "valueCoding": prod_coding
                }));

                match_parts.push(json!({
                    "name": "product",
                    "part": product_parts
                }));
            }

            params.push(json!({
                "name": "match",
                "part": match_parts
            }));
        }

        json!({
            "resourceType": "Parameters",
            "parameter": params
        })
    }
}

/// The $translate operation handler.
pub struct TranslateOperation;

impl TranslateOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<TranslateParams, OperationError> {
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut translate_params = TranslateParams::default();

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "code" => {
                            translate_params.code = param
                                .get("valueCode")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "system" => {
                            translate_params.system = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "version" => {
                            translate_params.version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "coding" => {
                            translate_params.coding = param.get("valueCoding").cloned();
                        }
                        "codeableConcept" => {
                            translate_params.codeable_concept =
                                param.get("valueCodeableConcept").cloned();
                        }
                        "source" => {
                            translate_params.source = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "target" => {
                            translate_params.target = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "url" | "conceptMap" => {
                            translate_params.concept_map_url = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "targetSystem" => {
                            translate_params.target_system = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "reverse" => {
                            translate_params.reverse = param
                                .get("valueBoolean")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                        }
                        _ => {}
                    }
                }
            }

            Ok(translate_params)
        } else {
            // Assume it's a flat object with query parameters
            let translate_params: TranslateParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok(translate_params)
        }
    }

    /// Extract code and system from params, with coding/codeableConcept fallback.
    fn resolve_code_system(params: &TranslateParams) -> Result<(String, String), OperationError> {
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

        // Try codeableConcept
        if let Some(ref cc) = params.codeable_concept {
            if let Some(codings) = cc.get("coding").and_then(|v| v.as_array()) {
                for coding in codings {
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
            }
        }

        // Fall back to explicit parameters
        let code = params.code.clone().ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'code' parameter (or 'coding' or 'codeableConcept')".into(),
            )
        })?;

        let system = params.system.clone().ok_or_else(|| {
            OperationError::InvalidParameters(
                "Missing 'system' parameter (or 'coding' with system)".into(),
            )
        })?;

        Ok((code, system))
    }

    /// Load a ConceptMap by URL from cache or canonical manager.
    async fn load_concept_map_by_url(&self, url: &str) -> Result<Value, OperationError> {
        let cache = get_cache();
        let cache_key = ConceptMapKey::new(url, None);

        // Check cache first
        if let Some(cached) = cache.get_concept_map(&cache_key).await {
            tracing::debug!(url = %url, "ConceptMap loaded from cache");
            return Ok(cached.as_ref().clone());
        }

        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("ConceptMap")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .map_err(|e| {
                OperationError::Internal(format!("Failed to search for ConceptMap: {}", e))
            })?;

        let result = search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content);

        // Cache the result
        cache.insert_concept_map(cache_key, result.clone()).await;

        result.ok_or_else(|| {
            OperationError::NotFound(format!("ConceptMap with url '{}' not found", url))
        })
    }

    /// Load a ConceptMap by ID from storage.
    async fn load_concept_map_by_id(
        &self,
        state: &AppState,
        id: &str,
    ) -> Result<Value, OperationError> {
        let result =
            state.storage.read("ConceptMap", id).await.map_err(|e| {
                OperationError::Internal(format!("Failed to read ConceptMap: {}", e))
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
                        .resource_type("ConceptMap")
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
                            OperationError::NotFound(format!("ConceptMap '{}' not found", id))
                        })
                } else {
                    Err(OperationError::NotFound(format!(
                        "ConceptMap '{}' not found",
                        id
                    )))
                }
            }
        }
    }

    /// Find ConceptMaps that can translate from source to target.
    async fn find_concept_maps(
        &self,
        source_system: &str,
        target_system: Option<&str>,
        concept_map_url: Option<&str>,
    ) -> Result<Vec<Value>, OperationError> {
        // If specific ConceptMap URL is provided, use only that
        if let Some(url) = concept_map_url {
            let cm = self.load_concept_map_by_url(url).await?;
            return Ok(vec![cm]);
        }

        // Otherwise, search for applicable ConceptMaps
        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

        let search_result = manager
            .search()
            .await
            .resource_type("ConceptMap")
            .limit(50)
            .execute()
            .await
            .map_err(|e| {
                OperationError::Internal(format!("Failed to search ConceptMaps: {}", e))
            })?;

        // Filter ConceptMaps by source and target
        let concept_maps: Vec<Value> = search_result
            .resources
            .into_iter()
            .filter(|r| {
                let content = &r.resource.content;

                // Check if ConceptMap applies to source system
                let source_matches = self.concept_map_matches_source(content, source_system);

                // Check if ConceptMap applies to target system (if specified)
                let target_matches = match target_system {
                    Some(target) => self.concept_map_matches_target(content, target),
                    None => true,
                };

                source_matches && target_matches
            })
            .map(|r| r.resource.content)
            .collect();

        Ok(concept_maps)
    }

    /// Check if a ConceptMap can translate from the given source system.
    fn concept_map_matches_source(&self, concept_map: &Value, source_system: &str) -> bool {
        // Check sourceUri/sourceCanonical
        if let Some(source) = concept_map
            .get("sourceUri")
            .or(concept_map.get("sourceCanonical"))
        {
            if source.as_str() == Some(source_system) {
                return true;
            }
        }

        // Check group[].source
        if let Some(groups) = concept_map.get("group").and_then(|v| v.as_array()) {
            for group in groups {
                if group.get("source").and_then(|v| v.as_str()) == Some(source_system) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if a ConceptMap can translate to the given target system.
    fn concept_map_matches_target(&self, concept_map: &Value, target_system: &str) -> bool {
        // Check targetUri/targetCanonical
        if let Some(target) = concept_map
            .get("targetUri")
            .or(concept_map.get("targetCanonical"))
        {
            if target.as_str() == Some(target_system) {
                return true;
            }
        }

        // Check group[].target
        if let Some(groups) = concept_map.get("group").and_then(|v| v.as_array()) {
            for group in groups {
                if group.get("target").and_then(|v| v.as_str()) == Some(target_system) {
                    return true;
                }
            }
        }

        false
    }

    /// Find mappings for a code in a ConceptMap.
    fn find_mappings(
        &self,
        concept_map: &Value,
        code: &str,
        source_system: &str,
        reverse: bool,
    ) -> Vec<TranslationMatch> {
        let mut matches = Vec::new();
        let cm_url = concept_map
            .get("url")
            .and_then(|v| v.as_str())
            .map(String::from);

        if let Some(groups) = concept_map.get("group").and_then(|v| v.as_array()) {
            for group in groups {
                let group_source = group.get("source").and_then(|v| v.as_str());
                let group_target = group.get("target").and_then(|v| v.as_str());

                // For normal translation, match on source; for reverse, match on target
                let matches_group = if reverse {
                    group_target == Some(source_system)
                } else {
                    group_source == Some(source_system)
                };

                if !matches_group {
                    continue;
                }

                if let Some(elements) = group.get("element").and_then(|v| v.as_array()) {
                    for element in elements {
                        let element_code = element.get("code").and_then(|v| v.as_str());

                        // For normal translation, check element code; for reverse, check target codes
                        if reverse {
                            // In reverse, we look for elements whose target matches our code
                            if let Some(targets) = element.get("target").and_then(|v| v.as_array())
                            {
                                for target in targets {
                                    if target.get("code").and_then(|v| v.as_str()) == Some(code) {
                                        // Found reverse match - the source element is our target
                                        let equivalence = self.reverse_equivalence(
                                            target
                                                .get("equivalence")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("equivalent"),
                                        );

                                        matches.push(TranslationMatch {
                                            equivalence,
                                            concept: TranslatedConcept {
                                                system: group_source.map(String::from),
                                                code: element_code.map(String::from),
                                                display: element
                                                    .get("display")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                            },
                                            source: cm_url.clone(),
                                            product: Vec::new(),
                                        });
                                    }
                                }
                            }
                        } else if element_code == Some(code) {
                            // Normal translation - element code matches, extract targets
                            if let Some(targets) = element.get("target").and_then(|v| v.as_array())
                            {
                                for target in targets {
                                    let equivalence = target
                                        .get("equivalence")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("equivalent")
                                        .to_string();

                                    let mut products = Vec::new();
                                    if let Some(prods) =
                                        target.get("product").and_then(|v| v.as_array())
                                    {
                                        for prod in prods {
                                            products.push(TranslationProduct {
                                                property: prod
                                                    .get("property")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string(),
                                                system: prod
                                                    .get("system")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                                code: prod
                                                    .get("code")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                                display: prod
                                                    .get("display")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                            });
                                        }
                                    }

                                    matches.push(TranslationMatch {
                                        equivalence,
                                        concept: TranslatedConcept {
                                            system: group_target.map(String::from),
                                            code: target
                                                .get("code")
                                                .and_then(|v| v.as_str())
                                                .map(String::from),
                                            display: target
                                                .get("display")
                                                .and_then(|v| v.as_str())
                                                .map(String::from),
                                        },
                                        source: cm_url.clone(),
                                        product: products,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        matches
    }

    /// Reverse an equivalence relationship.
    fn reverse_equivalence(&self, equivalence: &str) -> String {
        match equivalence {
            "wider" => "narrower".to_string(),
            "narrower" => "wider".to_string(),
            "specializes" => "subsumes".to_string(),
            "subsumes" => "specializes".to_string(),
            _ => equivalence.to_string(),
        }
    }

    /// Perform the translation operation.
    async fn translate(
        &self,
        state: &AppState,
        code: &str,
        source_system: &str,
        target_system: Option<&str>,
        concept_map_url: Option<&str>,
        concept_map_id: Option<&str>,
        reverse: bool,
    ) -> Result<TranslateResult, OperationError> {
        // Load ConceptMap(s)
        let concept_maps = if let Some(id) = concept_map_id {
            vec![self.load_concept_map_by_id(state, id).await?]
        } else {
            self.find_concept_maps(source_system, target_system, concept_map_url)
                .await?
        };

        if concept_maps.is_empty() {
            return Ok(TranslateResult::failure(format!(
                "No ConceptMap found for source system '{}'",
                source_system
            )));
        }

        // Search for mappings in each ConceptMap
        let mut all_matches = Vec::new();
        for cm in concept_maps {
            let matches = self.find_mappings(&cm, code, source_system, reverse);
            all_matches.extend(matches);
        }

        Ok(TranslateResult::success(all_matches))
    }
}

impl Default for TranslateOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for TranslateOperation {
    fn code(&self) -> &str {
        "translate"
    }

    /// Handle system-level $translate (POST /$translate).
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let translate_params = Self::extract_params(params)?;
        let (code, source_system) = Self::resolve_code_system(&translate_params)?;

        self.translate(
            state,
            &code,
            &source_system,
            translate_params
                .target_system
                .as_deref()
                .or(translate_params.target.as_deref()),
            translate_params.concept_map_url.as_deref(),
            None,
            translate_params.reverse,
        )
        .await
        .map(|r| r.to_parameters())
    }

    /// Handle type-level $translate (GET/POST /ConceptMap/$translate).
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "ConceptMap" {
            return Err(OperationError::NotSupported(format!(
                "$translate is only supported on ConceptMap, not {}",
                resource_type
            )));
        }

        let translate_params = Self::extract_params(params)?;
        let (code, source_system) = Self::resolve_code_system(&translate_params)?;

        self.translate(
            state,
            &code,
            &source_system,
            translate_params
                .target_system
                .as_deref()
                .or(translate_params.target.as_deref()),
            translate_params.concept_map_url.as_deref(),
            None,
            translate_params.reverse,
        )
        .await
        .map(|r| r.to_parameters())
    }

    /// Handle instance-level $translate (GET/POST /ConceptMap/{id}/$translate).
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "ConceptMap" {
            return Err(OperationError::NotSupported(format!(
                "$translate is only supported on ConceptMap, not {}",
                resource_type
            )));
        }

        let translate_params = Self::extract_params(params)?;
        let (code, source_system) = Self::resolve_code_system(&translate_params)?;

        self.translate(
            state,
            &code,
            &source_system,
            translate_params
                .target_system
                .as_deref()
                .or(translate_params.target.as_deref()),
            None,
            Some(id),
            translate_params.reverse,
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
            "code": "M",
            "system": "http://hl7.org/fhir/v3/AdministrativeGender",
            "target": "http://hl7.org/fhir/administrative-gender"
        });

        let translate_params = TranslateOperation::extract_params(&params).unwrap();
        assert_eq!(translate_params.code, Some("M".to_string()));
        assert_eq!(
            translate_params.system,
            Some("http://hl7.org/fhir/v3/AdministrativeGender".to_string())
        );
        assert_eq!(
            translate_params.target,
            Some("http://hl7.org/fhir/administrative-gender".to_string())
        );
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "code", "valueCode": "F"},
                {"name": "system", "valueUri": "http://example.org/gender"},
                {"name": "target", "valueUri": "http://example.org/gender2"}
            ]
        });

        let translate_params = TranslateOperation::extract_params(&params).unwrap();
        assert_eq!(translate_params.code, Some("F".to_string()));
        assert_eq!(
            translate_params.system,
            Some("http://example.org/gender".to_string())
        );
        assert_eq!(
            translate_params.target,
            Some("http://example.org/gender2".to_string())
        );
    }

    #[test]
    fn test_resolve_code_system() {
        let params = TranslateParams {
            code: Some("M".to_string()),
            system: Some("http://example.org".to_string()),
            ..Default::default()
        };

        let (code, system) = TranslateOperation::resolve_code_system(&params).unwrap();
        assert_eq!(code, "M");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_resolve_code_system_from_coding() {
        let params = TranslateParams {
            coding: Some(json!({
                "system": "http://example.org",
                "code": "F"
            })),
            ..Default::default()
        };

        let (code, system) = TranslateOperation::resolve_code_system(&params).unwrap();
        assert_eq!(code, "F");
        assert_eq!(system, "http://example.org");
    }

    #[test]
    fn test_translate_result_to_parameters_success() {
        let result = TranslateResult::success(vec![TranslationMatch {
            equivalence: "equivalent".to_string(),
            concept: TranslatedConcept {
                system: Some("http://example.org/target".to_string()),
                code: Some("male".to_string()),
                display: Some("Male".to_string()),
            },
            source: Some("http://example.org/ConceptMap/gender".to_string()),
            product: Vec::new(),
        }]);

        let params = result.to_parameters();
        assert_eq!(params["resourceType"], "Parameters");

        let parameters = params["parameter"].as_array().unwrap();
        assert!(
            parameters
                .iter()
                .any(|p| p["name"] == "result" && p["valueBoolean"] == true)
        );
        assert!(parameters.iter().any(|p| p["name"] == "match"));
    }

    #[test]
    fn test_translate_result_to_parameters_failure() {
        let result = TranslateResult::failure("No mapping found".to_string());

        let params = result.to_parameters();
        assert_eq!(params["resourceType"], "Parameters");

        let parameters = params["parameter"].as_array().unwrap();
        assert!(
            parameters
                .iter()
                .any(|p| p["name"] == "result" && p["valueBoolean"] == false)
        );
        assert!(parameters.iter().any(|p| p["name"] == "message"));
    }

    #[test]
    fn test_find_mappings() {
        let op = TranslateOperation::new();

        let concept_map = json!({
            "resourceType": "ConceptMap",
            "url": "http://example.org/ConceptMap/gender",
            "group": [{
                "source": "http://hl7.org/fhir/v3/AdministrativeGender",
                "target": "http://hl7.org/fhir/administrative-gender",
                "element": [
                    {
                        "code": "M",
                        "display": "Male (V3)",
                        "target": [{
                            "code": "male",
                            "display": "Male",
                            "equivalence": "equivalent"
                        }]
                    },
                    {
                        "code": "F",
                        "display": "Female (V3)",
                        "target": [{
                            "code": "female",
                            "display": "Female",
                            "equivalence": "equivalent"
                        }]
                    }
                ]
            }]
        });

        let matches = op.find_mappings(
            &concept_map,
            "M",
            "http://hl7.org/fhir/v3/AdministrativeGender",
            false,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].concept.code, Some("male".to_string()));
        assert_eq!(matches[0].equivalence, "equivalent");
    }

    #[test]
    fn test_reverse_equivalence() {
        let op = TranslateOperation::new();

        assert_eq!(op.reverse_equivalence("wider"), "narrower");
        assert_eq!(op.reverse_equivalence("narrower"), "wider");
        assert_eq!(op.reverse_equivalence("equivalent"), "equivalent");
    }
}
