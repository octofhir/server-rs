//! ValueSet $expand Operation
//!
//! Implements the FHIR ValueSet/$expand operation to expand a ValueSet
//! to its enumerated codes.
//!
//! Specification: http://hl7.org/fhir/valueset-operation-expand.html
//!
//! Supported invocation levels:
//! - System: `POST /$expand` with inline ValueSet or URL
//! - Type: `GET/POST /ValueSet/$expand` with url parameter
//! - Instance: `GET/POST /ValueSet/{id}/$expand`

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::canonical::get_manager;
use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

/// Parameters for the $expand operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandParams {
    /// Canonical URL of the ValueSet to expand
    pub url: Option<String>,

    /// ValueSet version
    pub value_set_version: Option<String>,

    /// Text filter to apply to code displays
    pub filter: Option<String>,

    /// Paging offset (0-based)
    #[serde(default)]
    pub offset: usize,

    /// Number of codes to return (default: unlimited)
    pub count: Option<usize>,

    /// Include designations in the expansion
    #[serde(default)]
    pub include_designations: bool,

    /// Include definition in the expansion
    #[serde(default)]
    pub include_definition: bool,

    /// Active codes only
    #[serde(default = "default_active_only")]
    pub active_only: bool,

    /// Exclude nested codes
    #[serde(default)]
    pub exclude_nested: bool,
}

fn default_active_only() -> bool {
    true
}

/// A code in the expansion.
#[derive(Debug, Clone, Serialize)]
pub struct ExpansionCode {
    pub system: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub designation: Vec<Value>,
}

/// The $expand operation handler.
pub struct ExpandOperation;

impl ExpandOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<(ExpandParams, Option<Value>), OperationError> {
        // Check if this is a Parameters resource
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut expand_params = ExpandParams::default();
            let mut inline_value_set: Option<Value> = None;

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "url" => {
                            expand_params.url =
                                param.get("valueUri").and_then(|v| v.as_str()).map(String::from);
                        }
                        "valueSetVersion" => {
                            expand_params.value_set_version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "filter" => {
                            expand_params.filter = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "offset" => {
                            expand_params.offset = param
                                .get("valueInteger")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0) as usize;
                        }
                        "count" => {
                            expand_params.count = param
                                .get("valueInteger")
                                .and_then(|v| v.as_i64())
                                .map(|v| v as usize);
                        }
                        "includeDesignations" => {
                            expand_params.include_designations = param
                                .get("valueBoolean")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                        }
                        "includeDefinition" => {
                            expand_params.include_definition = param
                                .get("valueBoolean")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                        }
                        "activeOnly" => {
                            expand_params.active_only = param
                                .get("valueBoolean")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true);
                        }
                        "excludeNested" => {
                            expand_params.exclude_nested = param
                                .get("valueBoolean")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                        }
                        "valueSet" => {
                            inline_value_set = param.get("resource").cloned();
                        }
                        _ => {}
                    }
                }
            }

            Ok((expand_params, inline_value_set))
        } else {
            // Assume it's a flat object with query parameters
            let expand_params: ExpandParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok((expand_params, None))
        }
    }

    /// Resolve a ValueSet by URL from the canonical manager.
    async fn resolve_value_set_by_url(url: &str) -> Result<Value, OperationError> {
        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

        // Search for the ValueSet by URL using canonical_pattern with escaped regex
        let escaped_url = regex::escape(url);
        let search_result = manager
            .search()
            .await
            .resource_type("ValueSet")
            .canonical_pattern(&format!("^{}$", escaped_url))
            .limit(10)
            .execute()
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to search for ValueSet: {}", e)))?;

        // Filter to exact URL match (canonical_pattern might match partial)
        search_result
            .resources
            .into_iter()
            .find(|r| r.resource.content.get("url").and_then(|v| v.as_str()) == Some(url))
            .map(|r| r.resource.content)
            .ok_or_else(|| OperationError::NotFound(format!("ValueSet with url '{}' not found", url)))
    }

    /// Load a ValueSet by ID from storage.
    async fn load_value_set_by_id(state: &AppState, id: &str) -> Result<Value, OperationError> {
        let result = state
            .storage
            .read("ValueSet", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to read ValueSet: {}", e)))?;

        match result {
            Some(stored) => Ok(stored.resource),
            None => {
                // Try canonical manager as fallback - search by text containing the ID
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

                    // Filter to find exact ID match
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

    /// Expand a ValueSet to its enumerated codes.
    async fn expand_value_set(
        &self,
        value_set: &Value,
        params: &ExpandParams,
    ) -> Result<Value, OperationError> {
        let mut codes: Vec<ExpansionCode> = Vec::new();
        let mut excluded_codes: HashSet<(String, String)> = HashSet::new();

        // Process compose.exclude first to build exclusion set
        if let Some(compose) = value_set.get("compose") {
            if let Some(excludes) = compose.get("exclude").and_then(|v| v.as_array()) {
                for exclude in excludes {
                    self.process_include_exclude(exclude, &mut excluded_codes, true)
                        .await;
                }
            }

            // Process compose.include
            if let Some(includes) = compose.get("include").and_then(|v| v.as_array()) {
                for include in includes {
                    self.process_include(include, &excluded_codes, &mut codes, params)
                        .await?;
                }
            }
        }

        // Apply text filter
        if let Some(ref filter) = params.filter {
            let filter_lower = filter.to_lowercase();
            codes.retain(|code| {
                code.display
                    .as_ref()
                    .map(|d| d.to_lowercase().contains(&filter_lower))
                    .unwrap_or(false)
                    || code.code.to_lowercase().contains(&filter_lower)
            });
        }

        // Calculate total before pagination
        let total = codes.len();

        // Apply pagination
        let offset = params.offset;
        let count = params.count.unwrap_or(codes.len());
        let paginated_codes: Vec<_> = codes.into_iter().skip(offset).take(count).collect();

        // Build expansion
        let expansion = self.build_expansion(value_set, &paginated_codes, total, offset, params);

        // Build response ValueSet
        let mut response = value_set.clone();
        response["expansion"] = expansion;

        // Remove compose from response if present (expansion replaces it conceptually)
        // Note: FHIR spec says we can keep compose, but many servers remove it for clarity

        Ok(response)
    }

    /// Process a compose.include element to gather codes.
    async fn process_include(
        &self,
        include: &Value,
        excluded: &HashSet<(String, String)>,
        codes: &mut Vec<ExpansionCode>,
        params: &ExpandParams,
    ) -> Result<(), OperationError> {
        let system = include
            .get("system")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let version = include.get("version").and_then(|v| v.as_str());

        // If concept list is provided, use those directly
        if let Some(concepts) = include.get("concept").and_then(|v| v.as_array()) {
            for concept in concepts {
                let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let display = concept.get("display").and_then(|v| v.as_str());

                // Check exclusion
                if excluded.contains(&(system.to_string(), code.to_string())) {
                    continue;
                }

                let mut designations = Vec::new();
                if params.include_designations {
                    if let Some(desigs) = concept.get("designation").and_then(|v| v.as_array()) {
                        designations = desigs.clone();
                    }
                }

                codes.push(ExpansionCode {
                    system: system.to_string(),
                    code: code.to_string(),
                    display: display.map(String::from),
                    version: version.map(String::from),
                    designation: designations,
                });
            }
        } else if let Some(filters) = include.get("filter").and_then(|v| v.as_array()) {
            // Filter-based inclusion - need to query CodeSystem
            self.process_filter_inclusion(system, version, filters, excluded, codes, params)
                .await?;
        } else if !system.is_empty() {
            // Include all codes from the system
            self.include_all_from_system(system, version, excluded, codes, params)
                .await?;
        }

        // Process valueSet references (import from other ValueSets)
        if let Some(value_sets) = include.get("valueSet").and_then(|v| v.as_array()) {
            for vs_url in value_sets {
                if let Some(url) = vs_url.as_str() {
                    match Self::resolve_value_set_by_url(url).await {
                        Ok(imported_vs) => {
                            // Recursively expand the imported ValueSet using Box::pin
                            // Note: In production, we'd want cycle detection here
                            let sub_params = ExpandParams {
                                include_designations: params.include_designations,
                                include_definition: params.include_definition,
                                active_only: params.active_only,
                                ..Default::default()
                            };
                            let expanded_result = Box::pin(self.expand_value_set(&imported_vs, &sub_params)).await;
                            if let Ok(expanded) = expanded_result {
                                if let Some(expansion) = expanded.get("expansion") {
                                    if let Some(contains) =
                                        expansion.get("contains").and_then(|v| v.as_array())
                                    {
                                        for contained in contains {
                                            let sys = contained
                                                .get("system")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let code = contained
                                                .get("code")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            if excluded.contains(&(sys.to_string(), code.to_string()))
                                            {
                                                continue;
                                            }

                                            codes.push(ExpansionCode {
                                                system: sys.to_string(),
                                                code: code.to_string(),
                                                display: contained
                                                    .get("display")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                                version: contained
                                                    .get("version")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                                designation: Vec::new(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(url = %url, error = %e, "Failed to import ValueSet");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Process filter-based code inclusion from a CodeSystem.
    async fn process_filter_inclusion(
        &self,
        system: &str,
        version: Option<&str>,
        filters: &[Value],
        excluded: &HashSet<(String, String)>,
        codes: &mut Vec<ExpansionCode>,
        params: &ExpandParams,
    ) -> Result<(), OperationError> {
        // Load the CodeSystem
        let code_system = self.load_code_system(system, version).await?;

        // Get all concepts from the CodeSystem
        let concepts = code_system
            .get("concept")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Apply filters
        for concept in &concepts {
            if self.concept_matches_filters(concept, filters) {
                let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");

                if excluded.contains(&(system.to_string(), code.to_string())) {
                    continue;
                }

                let mut designations = Vec::new();
                if params.include_designations {
                    if let Some(desigs) = concept.get("designation").and_then(|v| v.as_array()) {
                        designations = desigs.clone();
                    }
                }

                codes.push(ExpansionCode {
                    system: system.to_string(),
                    code: code.to_string(),
                    display: concept
                        .get("display")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    version: version.map(String::from),
                    designation: designations,
                });

                // Include nested concepts if not excluded
                if !params.exclude_nested {
                    self.collect_nested_concepts(
                        concept,
                        system,
                        version,
                        excluded,
                        codes,
                        params,
                    );
                }
            }
        }

        Ok(())
    }

    /// Include all codes from a CodeSystem.
    async fn include_all_from_system(
        &self,
        system: &str,
        version: Option<&str>,
        excluded: &HashSet<(String, String)>,
        codes: &mut Vec<ExpansionCode>,
        params: &ExpandParams,
    ) -> Result<(), OperationError> {
        let code_system = self.load_code_system(system, version).await?;

        let concepts = code_system
            .get("concept")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        for concept in &concepts {
            self.collect_concept_and_children(concept, system, version, excluded, codes, params);
        }

        Ok(())
    }

    /// Recursively collect a concept and its children.
    fn collect_concept_and_children(
        &self,
        concept: &Value,
        system: &str,
        version: Option<&str>,
        excluded: &HashSet<(String, String)>,
        codes: &mut Vec<ExpansionCode>,
        params: &ExpandParams,
    ) {
        let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");

        if !excluded.contains(&(system.to_string(), code.to_string())) {
            let mut designations = Vec::new();
            if params.include_designations {
                if let Some(desigs) = concept.get("designation").and_then(|v| v.as_array()) {
                    designations = desigs.clone();
                }
            }

            codes.push(ExpansionCode {
                system: system.to_string(),
                code: code.to_string(),
                display: concept
                    .get("display")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                version: version.map(String::from),
                designation: designations,
            });
        }

        // Process children if not excluding nested
        if !params.exclude_nested {
            if let Some(children) = concept.get("concept").and_then(|v| v.as_array()) {
                for child in children {
                    self.collect_concept_and_children(
                        child, system, version, excluded, codes, params,
                    );
                }
            }
        }
    }

    /// Collect nested concepts from a parent concept.
    fn collect_nested_concepts(
        &self,
        concept: &Value,
        system: &str,
        version: Option<&str>,
        excluded: &HashSet<(String, String)>,
        codes: &mut Vec<ExpansionCode>,
        params: &ExpandParams,
    ) {
        if let Some(children) = concept.get("concept").and_then(|v| v.as_array()) {
            for child in children {
                self.collect_concept_and_children(child, system, version, excluded, codes, params);
            }
        }
    }

    /// Check if a concept matches the given filters.
    fn concept_matches_filters(&self, concept: &Value, filters: &[Value]) -> bool {
        for filter in filters {
            let property = filter
                .get("property")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let op = filter.get("op").and_then(|v| v.as_str()).unwrap_or("");
            let value = filter.get("value").and_then(|v| v.as_str()).unwrap_or("");

            let matches = match (property, op) {
                ("code", "=") => concept.get("code").and_then(|v| v.as_str()) == Some(value),
                ("code", "in") => {
                    let codes: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
                    concept
                        .get("code")
                        .and_then(|v| v.as_str())
                        .map(|c| codes.contains(&c))
                        .unwrap_or(false)
                }
                ("display", "=") => concept.get("display").and_then(|v| v.as_str()) == Some(value),
                ("concept", "is-a") => {
                    // Check if concept is or descends from the specified code
                    self.is_or_descends_from(concept, value)
                }
                ("concept", "descendent-of") => {
                    // Check if concept descends from (but is not) the specified code
                    self.descends_from(concept, value)
                }
                _ => {
                    // Check custom properties
                    if let Some(props) = concept.get("property").and_then(|v| v.as_array()) {
                        props.iter().any(|p| {
                            p.get("code").and_then(|v| v.as_str()) == Some(property)
                                && match op {
                                    "=" => p.get("value").and_then(|v| v.as_str()) == Some(value),
                                    "regex" => {
                                        if let Some(prop_val) =
                                            p.get("value").and_then(|v| v.as_str())
                                        {
                                            regex::Regex::new(value)
                                                .map(|re| re.is_match(prop_val))
                                                .unwrap_or(false)
                                        } else {
                                            false
                                        }
                                    }
                                    _ => false,
                                }
                        })
                    } else {
                        true // If no properties, consider it a match (lenient)
                    }
                }
            };

            if !matches {
                return false;
            }
        }

        true
    }

    /// Check if a concept is or descends from a given code.
    fn is_or_descends_from(&self, concept: &Value, ancestor_code: &str) -> bool {
        let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");
        if code == ancestor_code {
            return true;
        }

        // Check parent property if available
        if let Some(props) = concept.get("property").and_then(|v| v.as_array()) {
            for prop in props {
                if prop.get("code").and_then(|v| v.as_str()) == Some("parent") {
                    if prop.get("valueCode").and_then(|v| v.as_str()) == Some(ancestor_code) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if a concept descends from (but is not) a given code.
    fn descends_from(&self, concept: &Value, ancestor_code: &str) -> bool {
        let code = concept.get("code").and_then(|v| v.as_str()).unwrap_or("");
        if code == ancestor_code {
            return false;
        }

        // Check parent property
        if let Some(props) = concept.get("property").and_then(|v| v.as_array()) {
            for prop in props {
                if prop.get("code").and_then(|v| v.as_str()) == Some("parent") {
                    if prop.get("valueCode").and_then(|v| v.as_str()) == Some(ancestor_code) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Load a CodeSystem by URL.
    async fn load_code_system(
        &self,
        system_url: &str,
        version: Option<&str>,
    ) -> Result<Value, OperationError> {
        let manager = get_manager()
            .ok_or_else(|| OperationError::Internal("Canonical manager not available".into()))?;

        // Search for CodeSystem by URL using canonical_pattern
        let escaped_url = regex::escape(system_url);
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

        // Filter to find exact URL match, and optionally version match
        search_result
            .resources
            .into_iter()
            .find(|r| {
                let content = &r.resource.content;
                let url_match = content.get("url").and_then(|v| v.as_str()) == Some(system_url);
                let version_match = match version {
                    Some(ver) => content.get("version").and_then(|v| v.as_str()) == Some(ver),
                    None => true,
                };
                url_match && version_match
            })
            .map(|r| r.resource.content)
            .ok_or_else(|| {
                OperationError::NotFound(format!("CodeSystem '{}' not found", system_url))
            })
    }

    /// Process include/exclude to build exclusion set.
    async fn process_include_exclude(
        &self,
        element: &Value,
        excluded: &mut HashSet<(String, String)>,
        _is_exclude: bool,
    ) {
        let system = element
            .get("system")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if let Some(concepts) = element.get("concept").and_then(|v| v.as_array()) {
            for concept in concepts {
                if let Some(code) = concept.get("code").and_then(|v| v.as_str()) {
                    excluded.insert((system.to_string(), code.to_string()));
                }
            }
        }
    }

    /// Build the expansion element for the response.
    fn build_expansion(
        &self,
        _value_set: &Value,
        codes: &[ExpansionCode],
        total: usize,
        offset: usize,
        params: &ExpandParams,
    ) -> Value {
        let contains: Vec<Value> = codes
            .iter()
            .map(|c| {
                let mut entry = json!({
                    "system": c.system,
                    "code": c.code,
                });

                if let Some(ref display) = c.display {
                    entry["display"] = json!(display);
                }

                if let Some(ref version) = c.version {
                    entry["version"] = json!(version);
                }

                if params.include_designations && !c.designation.is_empty() {
                    entry["designation"] = json!(c.designation);
                }

                entry
            })
            .collect();

        let mut expansion = json!({
            "identifier": format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            "timestamp": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "total": total,
            "offset": offset,
            "contains": contains,
        });

        // Add parameters used
        let mut exp_params = Vec::new();
        if let Some(ref filter) = params.filter {
            exp_params.push(json!({
                "name": "filter",
                "valueString": filter
            }));
        }
        if params.offset > 0 {
            exp_params.push(json!({
                "name": "offset",
                "valueInteger": params.offset
            }));
        }
        if let Some(count) = params.count {
            exp_params.push(json!({
                "name": "count",
                "valueInteger": count
            }));
        }
        if params.include_designations {
            exp_params.push(json!({
                "name": "includeDesignations",
                "valueBoolean": true
            }));
        }

        if !exp_params.is_empty() {
            expansion["parameter"] = json!(exp_params);
        }

        expansion
    }
}

impl Default for ExpandOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for ExpandOperation {
    fn code(&self) -> &str {
        "expand"
    }

    /// Handle system-level $expand (POST /$expand).
    async fn handle_system(
        &self,
        _state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let (expand_params, inline_value_set) = Self::extract_params(params)?;

        // Get the ValueSet to expand
        let value_set = if let Some(vs) = inline_value_set {
            vs
        } else if let Some(ref url) = expand_params.url {
            Self::resolve_value_set_by_url(url).await?
        } else {
            return Err(OperationError::InvalidParameters(
                "Either 'url' or 'valueSet' parameter is required".into(),
            ));
        };

        self.expand_value_set(&value_set, &expand_params).await
    }

    /// Handle type-level $expand (GET/POST /ValueSet/$expand).
    async fn handle_type(
        &self,
        _state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "ValueSet" {
            return Err(OperationError::NotSupported(format!(
                "$expand is only supported on ValueSet, not {}",
                resource_type
            )));
        }

        let (expand_params, inline_value_set) = Self::extract_params(params)?;

        let value_set = if let Some(vs) = inline_value_set {
            vs
        } else if let Some(ref url) = expand_params.url {
            Self::resolve_value_set_by_url(url).await?
        } else {
            return Err(OperationError::InvalidParameters(
                "Either 'url' or 'valueSet' parameter is required".into(),
            ));
        };

        self.expand_value_set(&value_set, &expand_params).await
    }

    /// Handle instance-level $expand (GET/POST /ValueSet/{id}/$expand).
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "ValueSet" {
            return Err(OperationError::NotSupported(format!(
                "$expand is only supported on ValueSet, not {}",
                resource_type
            )));
        }

        let (expand_params, _) = Self::extract_params(params)?;

        // Load the ValueSet by ID
        let value_set = Self::load_value_set_by_id(state, id).await?;

        self.expand_value_set(&value_set, &expand_params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_params_from_query() {
        let params = json!({
            "url": "http://example.org/fhir/ValueSet/test",
            "filter": "abc",
            "offset": 10,
            "count": 50
        });

        let (expand_params, inline_vs) = ExpandOperation::extract_params(&params).unwrap();
        assert_eq!(
            expand_params.url,
            Some("http://example.org/fhir/ValueSet/test".to_string())
        );
        assert_eq!(expand_params.filter, Some("abc".to_string()));
        assert_eq!(expand_params.offset, 10);
        assert_eq!(expand_params.count, Some(50));
        assert!(inline_vs.is_none());
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "url", "valueUri": "http://example.org/fhir/ValueSet/test"},
                {"name": "filter", "valueString": "xyz"},
                {"name": "offset", "valueInteger": 5}
            ]
        });

        let (expand_params, inline_vs) = ExpandOperation::extract_params(&params).unwrap();
        assert_eq!(
            expand_params.url,
            Some("http://example.org/fhir/ValueSet/test".to_string())
        );
        assert_eq!(expand_params.filter, Some("xyz".to_string()));
        assert_eq!(expand_params.offset, 5);
        assert!(inline_vs.is_none());
    }

    #[test]
    fn test_build_expansion() {
        let op = ExpandOperation::new();
        let codes = vec![
            ExpansionCode {
                system: "http://example.org".to_string(),
                code: "A".to_string(),
                display: Some("Code A".to_string()),
                version: None,
                designation: Vec::new(),
            },
            ExpansionCode {
                system: "http://example.org".to_string(),
                code: "B".to_string(),
                display: Some("Code B".to_string()),
                version: None,
                designation: Vec::new(),
            },
        ];

        let expansion = op.build_expansion(&json!({}), &codes, 2, 0, &ExpandParams::default());

        assert_eq!(expansion["total"], 2);
        assert_eq!(expansion["offset"], 0);
        assert!(expansion["contains"].as_array().unwrap().len() == 2);
    }
}
