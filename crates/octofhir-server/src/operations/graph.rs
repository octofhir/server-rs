//! $graph Operation Handler
//!
//! Implements the FHIR $graph operation for retrieving a graph of interconnected
//! resources defined by a GraphDefinition resource.
//!
//! The operation is invoked at instance level: `GET/POST [base]/[Resource]/[id]/$graph`
//! with a `graph` parameter referencing a GraphDefinition that defines which related
//! resources to include.
//!
//! # Algorithm
//!
//! 1. Resolve the GraphDefinition (by ID, canonical URL, or inline)
//! 2. Read the focal resource
//! 3. Traverse links defined in GraphDefinition using BFS:
//!    - **Forward links** (path set): extract references at the path, resolve them
//!    - **Reverse links** (params set): search for resources referencing the current one
//! 4. Return a Bundle of type "collection" with all discovered resources

use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashSet, VecDeque};

use super::handler::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_api::{Bundle, BundleEntry, RawJson};
use octofhir_core::fhir_reference::{FhirReference, parse_reference};
use octofhir_storage::{SearchParams, StoredResource};

// ---------------------------------------------------------------------------
// GraphDefinition model
// ---------------------------------------------------------------------------

/// Parsed representation of a GraphDefinition resource.
struct GraphDefinitionModel {
    /// The resource type where the graph starts (e.g. "Patient").
    start: String,
    /// Top-level link definitions.
    links: Vec<LinkModel>,
}

/// A single link in a GraphDefinition.
#[derive(Clone)]
struct LinkModel {
    /// Dot-notation path to follow (e.g. "Patient.managingOrganization").
    /// `None` means this is a reverse-reference link resolved via `target.params`.
    path: Option<String>,
    /// Target specifications.
    targets: Vec<TargetModel>,
}

/// A target resource type for a link.
#[derive(Clone)]
struct TargetModel {
    /// Target resource type (e.g. "Organization").
    resource_type: String,
    /// Search parameter name for reverse references (e.g. "patient").
    params: Option<String>,
    /// Nested links for recursive traversal.
    links: Vec<LinkModel>,
}

// ---------------------------------------------------------------------------
// GraphDefinition parsing
// ---------------------------------------------------------------------------

fn parse_graph_definition(value: &Value) -> Result<GraphDefinitionModel, OperationError> {
    let start = value
        .get("start")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            OperationError::InvalidParameters(
                "GraphDefinition is missing required 'start' field".into(),
            )
        })?
        .to_string();

    let links = value
        .get("link")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|l| parse_link(l).ok()).collect())
        .unwrap_or_default();

    Ok(GraphDefinitionModel { start, links })
}

fn parse_link(value: &Value) -> Result<LinkModel, OperationError> {
    let path = value.get("path").and_then(|v| v.as_str()).map(String::from);

    let targets = value
        .get("target")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|t| parse_target(t).ok()).collect())
        .unwrap_or_default();

    Ok(LinkModel { path, targets })
}

fn parse_target(value: &Value) -> Result<TargetModel, OperationError> {
    let resource_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            OperationError::InvalidParameters("GraphDefinition link target missing 'type'".into())
        })?
        .to_string();

    let params = value
        .get("params")
        .and_then(|v| v.as_str())
        .map(String::from);

    let links = value
        .get("link")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|l| parse_link(l).ok()).collect())
        .unwrap_or_default();

    Ok(TargetModel {
        resource_type,
        params,
        links,
    })
}

// ---------------------------------------------------------------------------
// JSON path resolution
// ---------------------------------------------------------------------------

/// Navigate a dot-separated path in a JSON value, flattening arrays at each level.
///
/// Returns all terminal values reached by following the path.
fn resolve_json_path<'a>(value: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut current = vec![value];

    for segment in path.split('.') {
        let mut next = Vec::new();
        for val in current {
            match val {
                Value::Object(map) => {
                    if let Some(child) = map.get(segment) {
                        match child {
                            Value::Array(arr) => next.extend(arr.iter()),
                            other => next.push(other),
                        }
                    }
                }
                Value::Array(arr) => {
                    for item in arr {
                        if let Some(child) = item.get(segment) {
                            match child {
                                Value::Array(inner) => next.extend(inner.iter()),
                                other => next.push(other),
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        current = next;
    }

    current
}

/// Extract FHIR references from a resource at a dot-notation path.
///
/// The path may start with a `ResourceType.` prefix (e.g. `"Patient.managingOrganization"`)
/// which is stripped before navigating the JSON.
fn extract_references_at_path(
    resource: &Value,
    path: &str,
    base_url: &str,
) -> Vec<FhirReference> {
    // Strip leading ResourceType. prefix if present
    let field_path = if let Some(dot_pos) = path.find('.') {
        let first_segment = &path[..dot_pos];
        if first_segment
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            &path[dot_pos + 1..]
        } else {
            path
        }
    } else {
        path
    };

    let values = resolve_json_path(resource, field_path);

    let mut refs = Vec::new();
    for val in values {
        // Reference object: { "reference": "Patient/123" }
        if let Some(ref_str) = val.get("reference").and_then(|r| r.as_str()) {
            if let Ok(parsed) = parse_reference(ref_str, Some(base_url)) {
                refs.push(parsed);
            }
        }
    }
    refs
}

// ---------------------------------------------------------------------------
// GraphDefinition lookup
// ---------------------------------------------------------------------------

/// Resolve the GraphDefinition from the operation parameters.
///
/// Supports:
/// - String value: looked up by ID, then by canonical URL
/// - Object value (inline POST): parsed directly
async fn resolve_graph_definition(
    state: &AppState,
    params: &Value,
) -> Result<GraphDefinitionModel, OperationError> {
    // Extract the "graph" parameter from the Parameters resource
    let graph_value = extract_param_value(params, "graph");

    match graph_value {
        Some(Value::String(ref s)) => resolve_graph_by_string(state, s).await,
        Some(Value::Object(_)) => {
            // Inline GraphDefinition (POST body)
            parse_graph_definition(graph_value.as_ref().unwrap())
        }
        _ => Err(OperationError::InvalidParameters(
            "Missing required 'graph' parameter. Provide a GraphDefinition ID or canonical URL."
                .into(),
        )),
    }
}

/// Look up a GraphDefinition by ID or canonical URL string.
async fn resolve_graph_by_string(
    state: &AppState,
    value: &str,
) -> Result<GraphDefinitionModel, OperationError> {
    // First try as a direct ID
    if !value.contains('/') && !value.starts_with("http") {
        if let Ok(Some(stored)) = state.storage.read("GraphDefinition", value).await {
            return parse_graph_definition(&stored.resource);
        }
    }

    // Try as a canonical URL
    if value.starts_with("http") {
        let search_params = SearchParams::new()
            .with_count(1)
            .with_param("url", value);
        if let Ok(result) = state.storage.search("GraphDefinition", &search_params).await {
            if let Some(entry) = result.entries.first() {
                return parse_graph_definition(&entry.resource);
            }
        }
    }

    // Try stripping "GraphDefinition/" prefix
    if let Some(id) = value.strip_prefix("GraphDefinition/") {
        if let Ok(Some(stored)) = state.storage.read("GraphDefinition", id).await {
            return parse_graph_definition(&stored.resource);
        }
    }

    Err(OperationError::NotFound(format!(
        "GraphDefinition '{}' not found",
        value
    )))
}

/// Extract a named parameter value from a FHIR Parameters resource.
fn extract_param_value(params: &Value, name: &str) -> Option<Value> {
    params
        .get("parameter")
        .and_then(|arr| arr.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .and_then(|p| {
            // Try valueString, valueUri, valueCanonical (all string types)
            p.get("valueString")
                .or_else(|| p.get("valueUri"))
                .or_else(|| p.get("valueCanonical"))
                .cloned()
                // Or an inline resource
                .or_else(|| p.get("resource").cloned())
        })
}

// ---------------------------------------------------------------------------
// Graph traversal (iterative BFS)
// ---------------------------------------------------------------------------

const MAX_DEPTH: usize = 10;
const MAX_RESOURCES: usize = 10_000;

/// A work item for BFS traversal.
struct WorkItem {
    resource: StoredResource,
    links: Vec<LinkModel>,
    depth: usize,
}

/// Traverse the resource graph starting from the focal resource.
async fn traverse(
    state: &AppState,
    focal: StoredResource,
    graph_def: &GraphDefinitionModel,
) -> Result<Vec<StoredResource>, OperationError> {
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut results: Vec<StoredResource> = Vec::new();
    let mut queue: VecDeque<WorkItem> = VecDeque::new();

    // Seed with focal resource
    visited.insert((focal.resource_type.clone(), focal.id.clone()));
    results.push(focal.clone());
    queue.push_back(WorkItem {
        resource: focal,
        links: graph_def.links.clone(),
        depth: 0,
    });

    while let Some(work) = queue.pop_front() {
        if work.depth >= MAX_DEPTH || results.len() >= MAX_RESOURCES {
            if work.depth >= MAX_DEPTH {
                tracing::warn!(depth = work.depth, "Graph traversal depth limit reached");
            } else {
                tracing::warn!(count = results.len(), "Graph traversal resource limit reached");
            }
            break;
        }

        for link in &work.links {
            for target in &link.targets {
                let resolved = if let Some(ref path) = link.path {
                    resolve_forward(state, &work.resource, path, target).await
                } else if let Some(ref params) = target.params {
                    resolve_reverse(state, &work.resource, &target.resource_type, params).await
                } else {
                    continue;
                };

                for res in resolved {
                    let key = (res.resource_type.clone(), res.id.clone());
                    if visited.insert(key) {
                        results.push(res.clone());
                        if !target.links.is_empty() {
                            queue.push_back(WorkItem {
                                resource: res,
                                links: target.links.clone(),
                                depth: work.depth + 1,
                            });
                        }
                    }

                    if results.len() >= MAX_RESOURCES {
                        break;
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Forward reference resolution: extract refs from the resource at the path.
async fn resolve_forward(
    state: &AppState,
    resource: &StoredResource,
    path: &str,
    target: &TargetModel,
) -> Vec<StoredResource> {
    let refs = extract_references_at_path(&resource.resource, path, &state.base_url);

    let mut results = Vec::new();
    for fhir_ref in refs {
        if fhir_ref.resource_type != target.resource_type {
            continue;
        }
        match state
            .storage
            .read(&fhir_ref.resource_type, &fhir_ref.id)
            .await
        {
            Ok(Some(stored)) => results.push(stored),
            Ok(None) => {
                tracing::debug!(
                    reference = %fhir_ref,
                    "Referenced resource not found during $graph traversal"
                );
            }
            Err(e) => {
                tracing::warn!(
                    reference = %fhir_ref,
                    error = %e,
                    "Error reading resource during $graph traversal"
                );
            }
        }
    }
    results
}

/// Reverse reference resolution: search for resources that reference the current one.
async fn resolve_reverse(
    state: &AppState,
    resource: &StoredResource,
    target_type: &str,
    params: &str,
) -> Vec<StoredResource> {
    let reference_value = format!("{}/{}", resource.resource_type, resource.id);

    let search_params = SearchParams::new()
        .with_count(1000)
        .with_param(params, &reference_value);

    match state.storage.search(target_type, &search_params).await {
        Ok(result) => result.entries,
        Err(e) => {
            tracing::warn!(
                target_type,
                params,
                error = %e,
                "Error searching reverse references during $graph traversal"
            );
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Bundle construction
// ---------------------------------------------------------------------------

fn build_collection_bundle(
    resources: Vec<StoredResource>,
    base_url: &str,
) -> Result<Value, OperationError> {
    let entries = resources
        .into_iter()
        .map(|stored| BundleEntry {
            full_url: Some(format!("{}/{}/{}", base_url, stored.resource_type, stored.id)),
            resource: Some(RawJson::from(stored.resource)),
            search: None,
            request: None,
            response: None,
        })
        .collect();

    let bundle = Bundle::collection(entries);

    serde_json::to_value(bundle)
        .map_err(|e| OperationError::Internal(format!("Failed to serialize bundle: {}", e)))
}

// ---------------------------------------------------------------------------
// OperationHandler
// ---------------------------------------------------------------------------

pub struct GraphOperation;

impl Default for GraphOperation {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphOperation {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl OperationHandler for GraphOperation {
    fn code(&self) -> &str {
        "graph"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // 1. Resolve GraphDefinition
        let graph_def = resolve_graph_definition(state, params).await?;

        // 2. Validate resource type matches GraphDefinition.start
        if graph_def.start != resource_type {
            return Err(OperationError::InvalidParameters(format!(
                "Resource type '{}' does not match GraphDefinition start type '{}'",
                resource_type, graph_def.start
            )));
        }

        // 3. Read the focal resource
        let focal = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // 4. Traverse the graph
        let resources = traverse(state, focal, &graph_def).await?;

        // 5. Build collection Bundle
        build_collection_bundle(resources, &state.base_url)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- resolve_json_path tests --

    #[test]
    fn test_resolve_simple_field() {
        let resource = json!({"name": "test"});
        let values = resolve_json_path(&resource, "name");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "test");
    }

    #[test]
    fn test_resolve_nested_field() {
        let resource = json!({"managingOrganization": {"reference": "Organization/1"}});
        let values = resolve_json_path(&resource, "managingOrganization.reference");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "Organization/1");
    }

    #[test]
    fn test_resolve_through_array() {
        let resource = json!({
            "name": [
                {"given": ["John", "James"]},
                {"given": ["Bob"]}
            ]
        });
        let values = resolve_json_path(&resource, "name.given");
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn test_resolve_missing_path() {
        let resource = json!({"name": "test"});
        let values = resolve_json_path(&resource, "nonexistent.field");
        assert!(values.is_empty());
    }

    #[test]
    fn test_resolve_reference_array() {
        let resource = json!({
            "result": [
                {"reference": "Observation/1"},
                {"reference": "Observation/2"}
            ]
        });
        let values = resolve_json_path(&resource, "result");
        assert_eq!(values.len(), 2);
    }

    // -- extract_references_at_path tests --

    #[test]
    fn test_extract_single_reference() {
        let resource = json!({
            "resourceType": "Patient",
            "managingOrganization": {"reference": "Organization/1"}
        });
        let refs = extract_references_at_path(&resource, "Patient.managingOrganization", "http://localhost");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].resource_type, "Organization");
        assert_eq!(refs[0].id, "1");
    }

    #[test]
    fn test_extract_array_references() {
        let resource = json!({
            "resourceType": "DiagnosticReport",
            "result": [
                {"reference": "Observation/1"},
                {"reference": "Observation/2"}
            ]
        });
        let refs =
            extract_references_at_path(&resource, "DiagnosticReport.result", "http://localhost");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].resource_type, "Observation");
        assert_eq!(refs[1].id, "2");
    }

    #[test]
    fn test_extract_strips_resource_type_prefix() {
        let resource = json!({
            "subject": {"reference": "Patient/123"}
        });
        let refs = extract_references_at_path(&resource, "Observation.subject", "http://localhost");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].resource_type, "Patient");
    }

    #[test]
    fn test_extract_skips_unresolvable_references() {
        let resource = json!({
            "subject": {"reference": "#contained-ref"}
        });
        let refs = extract_references_at_path(&resource, "Observation.subject", "http://localhost");
        assert!(refs.is_empty());
    }

    // -- parse_graph_definition tests --

    #[test]
    fn test_parse_simple_graph_definition() {
        let gd = json!({
            "resourceType": "GraphDefinition",
            "start": "Patient",
            "link": [{
                "path": "Patient.managingOrganization",
                "target": [{
                    "type": "Organization"
                }]
            }]
        });
        let model = parse_graph_definition(&gd).unwrap();
        assert_eq!(model.start, "Patient");
        assert_eq!(model.links.len(), 1);
        assert_eq!(
            model.links[0].path.as_deref(),
            Some("Patient.managingOrganization")
        );
        assert_eq!(model.links[0].targets.len(), 1);
        assert_eq!(model.links[0].targets[0].resource_type, "Organization");
    }

    #[test]
    fn test_parse_nested_links() {
        let gd = json!({
            "resourceType": "GraphDefinition",
            "start": "DiagnosticReport",
            "link": [{
                "path": "DiagnosticReport.result",
                "target": [{
                    "type": "Observation",
                    "link": [{
                        "path": "Observation.specimen",
                        "target": [{
                            "type": "Specimen"
                        }]
                    }]
                }]
            }]
        });
        let model = parse_graph_definition(&gd).unwrap();
        assert_eq!(model.start, "DiagnosticReport");
        let nested = &model.links[0].targets[0].links;
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].targets[0].resource_type, "Specimen");
    }

    #[test]
    fn test_parse_reverse_link() {
        let gd = json!({
            "resourceType": "GraphDefinition",
            "start": "Patient",
            "link": [{
                "target": [{
                    "type": "Observation",
                    "params": "patient"
                }]
            }]
        });
        let model = parse_graph_definition(&gd).unwrap();
        assert!(model.links[0].path.is_none());
        assert_eq!(
            model.links[0].targets[0].params.as_deref(),
            Some("patient")
        );
    }

    #[test]
    fn test_parse_missing_start() {
        let gd = json!({"resourceType": "GraphDefinition"});
        assert!(parse_graph_definition(&gd).is_err());
    }

    // -- extract_param_value tests --

    #[test]
    fn test_extract_param_string() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{"name": "graph", "valueString": "my-graph-def"}]
        });
        let val = extract_param_value(&params, "graph");
        assert_eq!(val.unwrap().as_str().unwrap(), "my-graph-def");
    }

    #[test]
    fn test_extract_param_uri() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{"name": "graph", "valueUri": "http://example.org/GraphDefinition/1"}]
        });
        let val = extract_param_value(&params, "graph");
        assert!(val.unwrap().as_str().unwrap().starts_with("http"));
    }

    #[test]
    fn test_extract_param_missing() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": []
        });
        assert!(extract_param_value(&params, "graph").is_none());
    }

    #[test]
    fn test_extract_param_inline_resource() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "graph",
                "resource": {
                    "resourceType": "GraphDefinition",
                    "start": "Patient"
                }
            }]
        });
        let val = extract_param_value(&params, "graph");
        assert!(val.unwrap().is_object());
    }
}
