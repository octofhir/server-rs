//! _include and _revinclude implementation for FHIR search.
//!
//! This module handles including referenced resources in search results.
//!
//! - `_include`: Include resources referenced by the search results
//! - `_revinclude`: Include resources that reference the search results
//! - `:iterate`: Follow chains of references

use crate::registry::SearchParameterRegistry;
use octofhir_core::fhir_reference::parse_reference_simple;
use std::collections::HashSet;

/// A parsed _include or _revinclude parameter.
#[derive(Debug, Clone)]
pub struct IncludeParam {
    /// The source resource type
    pub source_type: String,
    /// The search parameter name (reference parameter)
    pub search_param: String,
    /// Optional target type filter
    pub target_type: Option<String>,
    /// Whether to iterate (follow chains)
    pub iterate: bool,
    /// Whether this is a reverse include
    pub reverse: bool,
}

/// Error type for include operations.
#[derive(Debug, thiserror::Error)]
pub enum IncludeError {
    #[error("Invalid include parameter: {0}")]
    InvalidInclude(String),

    #[error("Unknown parameter {param} on {resource_type}")]
    UnknownParameter {
        param: String,
        resource_type: String,
    },
}

/// Check if a parameter name is an include parameter.
pub fn is_include_parameter(name: &str) -> bool {
    name == "_include" || name == "_include:iterate"
}

/// Check if a parameter name is a revinclude parameter.
pub fn is_revinclude_parameter(name: &str) -> bool {
    name == "_revinclude" || name == "_revinclude:iterate"
}

/// Parse an _include parameter value.
///
/// Format: `Type:searchParam` or `Type:searchParam:TargetType`
/// Special value: `*` includes all references
///
/// # Arguments
/// * `value` - The include value (e.g., "Observation:patient" or "Observation:patient:Patient")
/// * `iterate` - Whether :iterate modifier was used
/// * `registry` - The search parameter registry for validation
///
/// # Returns
/// An `IncludeParam` if parsing succeeds, or an error.
pub fn parse_include(
    value: &str,
    iterate: bool,
    registry: &SearchParameterRegistry,
) -> Result<IncludeParam, IncludeError> {
    // Handle wildcard
    if value == "*" {
        return Ok(IncludeParam {
            source_type: "*".to_string(),
            search_param: "*".to_string(),
            target_type: None,
            iterate,
            reverse: false,
        });
    }

    let parts: Vec<&str> = value.split(':').collect();

    if parts.len() < 2 {
        return Err(IncludeError::InvalidInclude(
            "_include requires Type:searchParam format".to_string(),
        ));
    }

    let source_type = parts[0];
    let search_param = parts[1];
    let target_type = parts.get(2).map(|s| (*s).to_string());

    // Validate the search parameter exists
    if registry.get(source_type, search_param).is_none() {
        return Err(IncludeError::UnknownParameter {
            param: search_param.to_string(),
            resource_type: source_type.to_string(),
        });
    }

    Ok(IncludeParam {
        source_type: source_type.to_string(),
        search_param: search_param.to_string(),
        target_type,
        iterate,
        reverse: false,
    })
}

/// Parse an _revinclude parameter value.
///
/// Format: `Type:searchParam` or `Type:searchParam:TargetType`
///
/// # Arguments
/// * `value` - The revinclude value (e.g., "Observation:patient")
/// * `iterate` - Whether :iterate modifier was used
/// * `registry` - The search parameter registry for validation
///
/// # Returns
/// An `IncludeParam` if parsing succeeds, or an error.
pub fn parse_revinclude(
    value: &str,
    iterate: bool,
    registry: &SearchParameterRegistry,
) -> Result<IncludeParam, IncludeError> {
    let mut include = parse_include(value, iterate, registry)?;
    include.reverse = true;
    Ok(include)
}

/// Extract all _include parameters from a query string.
pub fn extract_includes(query: &str, registry: &SearchParameterRegistry) -> Vec<IncludeParam> {
    let mut includes = Vec::new();

    for part in query.split('&') {
        if let Some((name, value)) = part.split_once('=') {
            let iterate = name == "_include:iterate";
            if (name == "_include" || iterate)
                && let Ok(include) = parse_include(value, iterate, registry)
            {
                includes.push(include);
            }
        }
    }

    includes
}

/// Extract all _revinclude parameters from a query string.
pub fn extract_revincludes(query: &str, registry: &SearchParameterRegistry) -> Vec<IncludeParam> {
    let mut includes = Vec::new();

    for part in query.split('&') {
        if let Some((name, value)) = part.split_once('=') {
            let iterate = name == "_revinclude:iterate";
            if (name == "_revinclude" || iterate)
                && let Ok(include) = parse_revinclude(value, iterate, registry)
            {
                includes.push(include);
            }
        }
    }

    includes
}

/// Build SQL to fetch included resources for forward includes.
///
/// This generates a query to fetch resources referenced by the main results.
pub fn build_include_query(
    include: &IncludeParam,
    source_ids: &[String],
    ref_path: &str,
) -> Option<(String, Vec<String>)> {
    if source_ids.is_empty() {
        return None;
    }

    // For wildcard target, we would need to extract type from reference and query multiple tables
    // For now, return None for untyped includes
    let target_table = include.target_type.as_ref()?.to_lowercase();

    let placeholders: Vec<String> = source_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("${}", i + 1))
        .collect();

    let sql = format!(
        "SELECT id, txid, ts, resource_type, resource, status \
         FROM {target_table} \
         WHERE id::text IN (\
             SELECT fhir_ref_id({ref_path}->>'reference') \
             FROM {source_table} \
             WHERE id::text IN ({placeholders})\
         ) AND status != 'deleted'",
        source_table = include.source_type.to_lowercase(),
        placeholders = placeholders.join(", ")
    );

    Some((sql, source_ids.to_vec()))
}

/// Build SQL to fetch reverse included resources.
///
/// This generates a query to fetch resources that reference the main results.
pub fn build_revinclude_query(
    include: &IncludeParam,
    target_type: &str,
    target_ids: &[String],
    ref_path: &str,
) -> Option<(String, Vec<String>)> {
    if target_ids.is_empty() {
        return None;
    }

    let source_table = include.source_type.to_lowercase();

    // Build reference values to search for
    let ref_values: Vec<String> = target_ids
        .iter()
        .map(|id| format!("{target_type}/{id}"))
        .collect();

    let placeholders: String = ref_values
        .iter()
        .enumerate()
        .map(|(i, _)| format!("${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "SELECT id, txid, ts, resource_type, resource, status \
         FROM {source_table} \
         WHERE {ref_path}->>'reference' IN ({placeholders}) \
         AND status != 'deleted'"
    );

    Some((sql, ref_values))
}

// ============================================================================
// _include:iterate Execution with Cycle Detection
// ============================================================================

/// A resource key for cycle detection (resource_type, id).
pub type ResourceKey = (String, String);

/// An included resource to add to the search bundle.
#[derive(Debug, Clone)]
pub struct IncludedResource {
    pub resource_type: String,
    pub id: String,
    pub resource: serde_json::Value,
    /// "include" for _include results, "match" for main search results
    pub search_mode: String,
}

/// Context for tracking include iteration state.
#[derive(Debug)]
pub struct IncludeContext {
    /// Resources already visited (for cycle detection)
    visited: HashSet<ResourceKey>,
    /// Maximum iterations for :iterate (prevents infinite loops)
    max_iterations: usize,
    /// Current iteration depth
    current_iteration: usize,
}

impl Default for IncludeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl IncludeContext {
    /// Create a new include context with default max iterations (10).
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            max_iterations: 10,
            current_iteration: 0,
        }
    }

    /// Create context with custom max iterations.
    pub fn with_max_iterations(max: usize) -> Self {
        Self {
            visited: HashSet::new(),
            max_iterations: max,
            current_iteration: 0,
        }
    }

    /// Check if a resource has been visited.
    pub fn is_visited(&self, resource_type: &str, id: &str) -> bool {
        self.visited
            .contains(&(resource_type.to_string(), id.to_string()))
    }

    /// Mark a resource as visited. Returns false if already visited.
    pub fn mark_visited(&mut self, resource_type: &str, id: &str) -> bool {
        self.visited
            .insert((resource_type.to_string(), id.to_string()))
    }

    /// Check if we can continue iterating.
    pub fn can_iterate(&self) -> bool {
        self.current_iteration < self.max_iterations
    }

    /// Increment iteration counter.
    pub fn next_iteration(&mut self) {
        self.current_iteration += 1;
    }

    /// Get all visited resource keys.
    pub fn visited_keys(&self) -> &HashSet<ResourceKey> {
        &self.visited
    }

    /// Number of visited resources.
    pub fn visited_count(&self) -> usize {
        self.visited.len()
    }
}

/// Extract reference values from a FHIR resource JSON.
///
/// Returns a list of (target_type, target_id) pairs for all references found.
pub fn extract_references_from_resource(
    resource: &serde_json::Value,
    ref_paths: &[&str],
) -> Vec<(String, String)> {
    let mut refs = Vec::new();

    for path in ref_paths {
        if let Some(ref_value) = get_reference_at_path(resource, path)
            && let Some((rtype, rid)) = parse_reference_value(&ref_value)
        {
            refs.push((rtype, rid));
        }
    }

    refs
}

/// Get reference value at a dot-separated path in JSON.
fn get_reference_at_path(resource: &serde_json::Value, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = resource;

    // Skip resource type prefix (e.g., "Observation.subject" -> "subject")
    let start = if parts.len() > 1 && current.get(parts[0]).is_none() {
        1
    } else {
        0
    };

    for part in &parts[start..] {
        current = current.get(part)?;
    }

    // Handle Reference type (has "reference" field)
    if let Some(ref_str) = current.get("reference").and_then(|v| v.as_str()) {
        return Some(ref_str.to_string());
    }

    // Direct string value
    current.as_str().map(String::from)
}

/// Parse a FHIR reference value into (type, id).
///
/// Handles formats: "Patient/123", "http://example.org/fhir/Patient/123"
///
/// Note: This function treats all absolute URLs as local since it doesn't have
/// access to the server's base URL. For proper base URL validation, use
/// `octofhir_core::fhir_reference::parse_reference()` directly with the base URL.
pub fn parse_reference_value(reference: &str) -> Option<(String, String)> {
    // For backwards compatibility, treat absolute URLs as local by extracting
    // the last two path segments (Type/id). This matches the old behavior.
    // For more robust handling with base_url validation, callers should use
    // parse_reference() directly from octofhir_core::fhir_reference.
    if reference.contains("://") {
        // Extract Type/id from the end of the URL
        let (prefix, id) = reference.rsplit_once('/')?;
        let (_, rtype) = prefix.rsplit_once('/')?;
        if rtype.is_empty() || id.is_empty() {
            return None;
        }
        // Validate resource type starts with uppercase
        if !rtype.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
            return None;
        }
        return Some((rtype.to_string(), id.to_string()));
    }

    // For relative references, delegate to the shared implementation
    parse_reference_simple(reference, None).ok()
}

/// Plan for executing includes with iteration.
#[derive(Debug)]
pub struct IncludePlan {
    /// Forward includes to execute
    pub includes: Vec<IncludeParam>,
    /// Reverse includes to execute
    pub revincludes: Vec<IncludeParam>,
    /// Initial resource IDs by type
    pub initial_resources: Vec<(String, String)>,
}

impl IncludePlan {
    /// Create a new include plan.
    pub fn new(
        includes: Vec<IncludeParam>,
        revincludes: Vec<IncludeParam>,
        initial_resources: Vec<(String, String)>,
    ) -> Self {
        Self {
            includes,
            revincludes,
            initial_resources,
        }
    }

    /// Check if any includes have :iterate.
    pub fn has_iterate(&self) -> bool {
        self.includes.iter().any(|i| i.iterate) || self.revincludes.iter().any(|i| i.iterate)
    }

    /// Get non-iterate includes.
    pub fn non_iterate_includes(&self) -> Vec<&IncludeParam> {
        self.includes.iter().filter(|i| !i.iterate).collect()
    }

    /// Get iterate includes.
    pub fn iterate_includes(&self) -> Vec<&IncludeParam> {
        self.includes.iter().filter(|i| i.iterate).collect()
    }
}

/// Generate SQL queries for a batch of resources to include.
///
/// Returns list of (sql, params, target_type) tuples.
pub fn generate_include_queries(
    include: &IncludeParam,
    source_ids: &[String],
    ref_path: &str,
    registry: &SearchParameterRegistry,
) -> Vec<(String, Vec<String>, String)> {
    let mut queries = Vec::new();

    if source_ids.is_empty() {
        return queries;
    }

    // Determine target types
    let target_types: Vec<String> = if let Some(ref target) = include.target_type {
        vec![target.clone()]
    } else if let Some(param) = registry.get(&include.source_type, &include.search_param) {
        param.target.clone()
    } else {
        return queries;
    };

    let source_table = include.source_type.to_lowercase();

    for target_type in target_types {
        let target_table = target_type.to_lowercase();
        let placeholders: String = source_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT id, txid, ts, resource_type, resource, status \
             FROM {target_table} \
             WHERE id::text IN (\
                 SELECT fhir_ref_id({ref_path}->>'reference') \
                 FROM {source_table} \
                 WHERE id::text IN ({placeholders}) \
                 AND fhir_ref_type({ref_path}->>'reference') = '{target_type}'\
             ) AND status != 'deleted'"
        );

        queries.push((sql, source_ids.to_vec(), target_type));
    }

    queries
}

/// Generate SQL queries for reverse includes.
pub fn generate_revinclude_queries(
    include: &IncludeParam,
    target_type: &str,
    target_ids: &[String],
    ref_path: &str,
) -> Vec<(String, Vec<String>, String)> {
    if target_ids.is_empty() {
        return vec![];
    }

    let source_table = include.source_type.to_lowercase();

    let ref_values: Vec<String> = target_ids
        .iter()
        .map(|id| format!("{target_type}/{id}"))
        .collect();

    let placeholders: String = ref_values
        .iter()
        .enumerate()
        .map(|(i, _)| format!("${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "SELECT id, txid, ts, resource_type, resource, status \
         FROM {source_table} \
         WHERE {ref_path}->>'reference' IN ({placeholders}) \
         AND status != 'deleted'"
    );

    vec![(sql, ref_values, include.source_type.clone())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{SearchParameter, SearchParameterType};

    fn create_test_registry() -> SearchParameterRegistry {
        let mut registry = SearchParameterRegistry::new();

        // Observation.patient -> Patient
        let patient_param = SearchParameter::new(
            "patient",
            "http://hl7.org/fhir/SearchParameter/Observation-patient",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject.where(resolve() is Patient)")
        .with_targets(vec!["Patient".to_string()]);
        registry.register(patient_param);

        // Observation.subject -> multiple types
        let subject_param = SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".to_string(), "Group".to_string()]);
        registry.register(subject_param);

        registry
    }

    #[test]
    fn test_is_include_parameter() {
        assert!(is_include_parameter("_include"));
        assert!(is_include_parameter("_include:iterate"));
        assert!(!is_include_parameter("_revinclude"));
        assert!(!is_include_parameter("patient"));
    }

    #[test]
    fn test_is_revinclude_parameter() {
        assert!(is_revinclude_parameter("_revinclude"));
        assert!(is_revinclude_parameter("_revinclude:iterate"));
        assert!(!is_revinclude_parameter("_include"));
    }

    #[test]
    fn test_parse_include_simple() {
        let registry = create_test_registry();
        let result = parse_include("Observation:patient", false, &registry);

        assert!(result.is_ok());
        let include = result.unwrap();
        assert_eq!(include.source_type, "Observation");
        assert_eq!(include.search_param, "patient");
        assert_eq!(include.target_type, None);
        assert!(!include.iterate);
        assert!(!include.reverse);
    }

    #[test]
    fn test_parse_include_with_target() {
        let registry = create_test_registry();
        let result = parse_include("Observation:subject:Patient", false, &registry);

        assert!(result.is_ok());
        let include = result.unwrap();
        assert_eq!(include.source_type, "Observation");
        assert_eq!(include.search_param, "subject");
        assert_eq!(include.target_type, Some("Patient".to_string()));
    }

    #[test]
    fn test_parse_include_iterate() {
        let registry = create_test_registry();
        let result = parse_include("Observation:patient", true, &registry);

        assert!(result.is_ok());
        let include = result.unwrap();
        assert!(include.iterate);
    }

    #[test]
    fn test_parse_include_wildcard() {
        let registry = create_test_registry();
        let result = parse_include("*", false, &registry);

        assert!(result.is_ok());
        let include = result.unwrap();
        assert_eq!(include.source_type, "*");
        assert_eq!(include.search_param, "*");
    }

    #[test]
    fn test_parse_include_invalid() {
        let registry = create_test_registry();
        let result = parse_include("Observation", false, &registry);

        assert!(result.is_err());
        assert!(matches!(result, Err(IncludeError::InvalidInclude(_))));
    }

    #[test]
    fn test_parse_revinclude() {
        let registry = create_test_registry();
        let result = parse_revinclude("Observation:patient", false, &registry);

        assert!(result.is_ok());
        let include = result.unwrap();
        assert!(include.reverse);
    }

    #[test]
    fn test_extract_includes() {
        let registry = create_test_registry();
        let query = "_include=Observation:patient&name=Smith&_include:iterate=Observation:subject";
        let includes = extract_includes(query, &registry);

        assert_eq!(includes.len(), 2);
        assert!(!includes[0].iterate);
        assert!(includes[1].iterate);
    }

    // ========================================
    // Cycle Detection Tests
    // ========================================

    #[test]
    fn test_include_context_cycle_detection() {
        let mut ctx = IncludeContext::new();

        // First visit should succeed
        assert!(ctx.mark_visited("Patient", "123"));
        assert!(ctx.is_visited("Patient", "123"));
        assert_eq!(ctx.visited_count(), 1);

        // Second visit to same resource should return false
        assert!(!ctx.mark_visited("Patient", "123"));

        // Different resource should succeed
        assert!(ctx.mark_visited("Observation", "456"));
        assert_eq!(ctx.visited_count(), 2);
    }

    #[test]
    fn test_include_context_max_iterations() {
        let mut ctx = IncludeContext::with_max_iterations(3);

        assert!(ctx.can_iterate());
        ctx.next_iteration();
        assert!(ctx.can_iterate());
        ctx.next_iteration();
        assert!(ctx.can_iterate());
        ctx.next_iteration();
        assert!(!ctx.can_iterate());
    }

    #[test]
    fn test_parse_reference_value_simple() {
        let result = parse_reference_value("Patient/123");
        assert_eq!(result, Some(("Patient".to_string(), "123".to_string())));

        let result = parse_reference_value("Observation/abc-def");
        assert_eq!(
            result,
            Some(("Observation".to_string(), "abc-def".to_string()))
        );
    }

    #[test]
    fn test_parse_reference_value_absolute_url() {
        let result = parse_reference_value("http://example.org/fhir/Patient/123");
        assert_eq!(result, Some(("Patient".to_string(), "123".to_string())));
    }

    #[test]
    fn test_parse_reference_value_invalid() {
        assert!(parse_reference_value("invalid").is_none());
        assert!(parse_reference_value("/123").is_none());
        assert!(parse_reference_value("Patient/").is_none());
    }

    #[test]
    fn test_extract_references_from_resource() {
        let resource = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {
                "reference": "Patient/123"
            },
            "performer": [{
                "reference": "Practitioner/456"
            }]
        });

        let refs = extract_references_from_resource(&resource, &["subject"]);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], ("Patient".to_string(), "123".to_string()));
    }

    #[test]
    fn test_include_plan_has_iterate() {
        let registry = create_test_registry();
        let include = parse_include("Observation:patient", true, &registry).unwrap();
        let plan = IncludePlan::new(vec![include], vec![], vec![]);
        assert!(plan.has_iterate());

        let include = parse_include("Observation:patient", false, &registry).unwrap();
        let plan = IncludePlan::new(vec![include], vec![], vec![]);
        assert!(!plan.has_iterate());
    }

    #[test]
    fn test_include_plan_filter_iterate() {
        let registry = create_test_registry();
        let include1 = parse_include("Observation:patient", false, &registry).unwrap();
        let include2 = parse_include("Observation:subject", true, &registry).unwrap();
        let plan = IncludePlan::new(vec![include1, include2], vec![], vec![]);

        assert_eq!(plan.non_iterate_includes().len(), 1);
        assert_eq!(plan.iterate_includes().len(), 1);
        assert_eq!(plan.iterate_includes()[0].search_param, "subject");
    }
}
