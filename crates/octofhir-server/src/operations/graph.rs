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

use crate::operations::handler::OperationLevel;

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
    profile: Option<String>,
    /// Top-level link definitions.
    links: Vec<LinkModel>,
}

/// A single link in a GraphDefinition.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LinkModel {
    /// Dot-notation path to follow (e.g. "Patient.managingOrganization").
    /// `None` means this is a reverse-reference link resolved via `target.params`.
    path: Option<String>,
    min: Option<usize>,
    max: Option<usize>,
    /// Target specifications.
    targets: Vec<TargetModel>,
}

/// A target resource type for a link.
#[derive(Clone, PartialEq, Eq, Hash)]
struct TargetModel {
    /// Target resource type (e.g. "Organization").
    resource_type: String,
    /// Search parameter name for reverse references (e.g. "patient").
    params: Option<String>,
    profile: Option<String>,
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

    let links = array(value, "link")?
        .iter()
        .map(parse_link)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(GraphDefinitionModel {
        start,
        profile: optional_string(value, "profile")?,
        links,
    })
}

fn optional_string(value: &Value, name: &str) -> Result<Option<String>, OperationError> {
    value
        .get(name)
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(String::from)
                .ok_or_else(|| {
                    OperationError::InvalidParameters(format!(
                        "GraphDefinition {name} must be a nonempty string"
                    ))
                })
        })
        .transpose()
}

fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], OperationError> {
    match value.get(name) {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        Some(_) => Err(OperationError::InvalidParameters(format!(
            "GraphDefinition {name} must be an array"
        ))),
    }
}

fn parse_link(value: &Value) -> Result<LinkModel, OperationError> {
    let path = optional_string(value, "path")?;
    if let Some(path) = &path
        && path != "*"
    {
        let ast = octofhir_fhirpath::parse_expression(path).map_err(|error| {
            OperationError::InvalidParameters(format!("Invalid GraphDefinition path: {error}"))
        })?;
        if contains_resolve(&ast) {
            return Err(OperationError::InvalidParameters(
                "GraphDefinition path must not invoke resolve()".into(),
            ));
        }
    }
    // These constraints require slice and compartment rule evaluation; never
    // return a successful graph after silently ignoring them.
    if value.get("sliceName").is_some() {
        return Err(OperationError::NotImplemented(
            "GraphDefinition sliceName constraints are unsupported".into(),
        ));
    }
    let min = value
        .get("min")
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "GraphDefinition min requires a nonnegative integer".into(),
                    )
                })
        })
        .transpose()?;
    let max = optional_string(value, "max")?
        .map(|value| {
            if value == "*" {
                Ok(None)
            } else {
                value.parse::<usize>().map(Some).map_err(|_| {
                    OperationError::InvalidParameters(
                        "GraphDefinition max requires a nonnegative integer or *".into(),
                    )
                })
            }
        })
        .transpose()?
        .flatten();
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return Err(OperationError::InvalidParameters(
            "GraphDefinition min exceeds max".into(),
        ));
    }

    let targets = array(value, "target")?
        .iter()
        .map(parse_target)
        .collect::<Result<Vec<_>, _>>()?;

    if path.is_none()
        && targets.iter().any(|target| {
            target
                .params
                .as_deref()
                .is_none_or(|params| !params.contains("{ref}"))
        })
    {
        return Err(OperationError::InvalidParameters(
            "GraphDefinition reverse link requires target.params containing {ref}".into(),
        ));
    }

    Ok(LinkModel {
        path,
        min,
        max,
        targets,
    })
}

fn parse_target(value: &Value) -> Result<TargetModel, OperationError> {
    let resource_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            OperationError::InvalidParameters("GraphDefinition link target missing 'type'".into())
        })?
        .to_string();

    let params = optional_string(value, "params")?;
    if value
        .get("compartment")
        .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
    {
        return Err(OperationError::NotImplemented(
            "GraphDefinition compartment constraints are unsupported".into(),
        ));
    }

    let links = array(value, "link")?
        .iter()
        .map(parse_link)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TargetModel {
        resource_type,
        params,
        profile: optional_string(value, "profile")?,
        links,
    })
}

fn contains_resolve(node: &octofhir_fhirpath::ast::ExpressionNode) -> bool {
    use octofhir_fhirpath::ast::ExpressionNode as E;
    match node {
        E::FunctionCall(node) => {
            node.name == "resolve" || node.arguments.iter().any(contains_resolve)
        }
        E::MethodCall(node) => {
            node.method == "resolve"
                || contains_resolve(&node.object)
                || node.arguments.iter().any(contains_resolve)
        }
        E::PropertyAccess(node) => contains_resolve(&node.object),
        E::IndexAccess(node) => contains_resolve(&node.object) || contains_resolve(&node.index),
        E::BinaryOperation(node) => contains_resolve(&node.left) || contains_resolve(&node.right),
        E::UnaryOperation(node) => contains_resolve(&node.operand),
        E::Lambda(node) => contains_resolve(&node.body),
        E::Collection(node) => node.elements.iter().any(contains_resolve),
        E::Parenthesized(node) => contains_resolve(node),
        E::TypeCast(node) => contains_resolve(&node.expression),
        E::Filter(node) => contains_resolve(&node.base) || contains_resolve(&node.condition),
        E::Union(node) => contains_resolve(&node.left) || contains_resolve(&node.right),
        E::TypeCheck(node) => contains_resolve(&node.expression),
        E::Path(node) => contains_resolve(&node.base),
        E::Literal(_) | E::Identifier(_) | E::Variable(_) | E::TypeInfo(_) => false,
    }
}

// ---------------------------------------------------------------------------
// JSON path resolution
// ---------------------------------------------------------------------------

/// Navigate a dot-separated path in a JSON value, flattening arrays at each level.
///
/// Returns all terminal values reached by following the path.
#[cfg(test)]
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
#[cfg(test)]
fn collect_references_at_path(resource: &Value, path: &str, base_url: &str) -> Vec<FhirReference> {
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
        if let Some(ref_str) = val.get("reference").and_then(|r| r.as_str())
            && let Ok(parsed) = parse_reference(ref_str, Some(base_url))
        {
            refs.push(parsed);
        }
    }
    refs
}

async fn link_references(
    state: &AppState,
    resource: &Value,
    path: &str,
    remapper: &mut crate::bundle_references::BundleReferenceRemapper,
) -> Result<Vec<FhirReference>, OperationError> {
    let references = if path == "*" {
        remapper
            .references(state.model_provider.as_ref(), resource)
            .await?
    } else {
        let collection = octofhir_fhirpath::Collection::from_json_resource(
            resource.clone(),
            Some(state.model_provider.clone()),
        )
        .await
        .map_err(|error| {
            OperationError::InvalidParameters(format!("Cannot evaluate graph path: {error}"))
        })?;
        let evaluation = octofhir_fhirpath::EvaluationContext::new(
            collection,
            state.model_provider.clone(),
            None,
            None,
            None,
        );
        let result = state
            .fhirpath_engine
            .evaluate(path, &evaluation)
            .await
            .map_err(|error| {
                OperationError::InvalidParameters(format!("Cannot evaluate graph path: {error}"))
            })?;
        let mut references = Vec::new();
        for value in result.value {
            let octofhir_fhirpath::FhirPathValue::Resource(resource, info, _) = value else {
                return Err(OperationError::InvalidParameters(
                    "GraphDefinition path must select References".into(),
                ));
            };
            if info.name.as_deref().unwrap_or(&info.type_name) != "Reference" {
                return Err(OperationError::InvalidParameters(
                    "GraphDefinition path must select References".into(),
                ));
            }
            if let Some(reference) = resource.to_json()["reference"].as_str() {
                references.push(reference.to_owned());
            }
        }
        references
    };
    Ok(references
        .into_iter()
        .filter_map(|reference| parse_reference(&reference, Some(&state.fhir_base_url())).ok())
        .collect())
}

async fn matches_profile(
    state: &AppState,
    resource: &Value,
    profile: Option<&str>,
) -> Result<bool, OperationError> {
    let Some(profile) = profile else {
        return Ok(true);
    };
    if state
        .model_provider
        .get_schema_by_url(profile)
        .await
        .is_none()
    {
        return Err(OperationError::NotImplemented(format!(
            "GraphDefinition profile is unavailable: {profile}"
        )));
    }
    Ok(state
        .validation_service
        .validate_against_profiles(resource, &[profile.to_owned()])
        .await
        .valid)
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
    context: &crate::operations::OperationContext,
    params: &Value,
) -> Result<GraphDefinitionModel, OperationError> {
    // Extract the "graph" parameter from the Parameters resource
    let graph_value = extract_param_value(params, "graph");

    match graph_value {
        Some(Value::String(ref s)) => resolve_graph_by_string(state, context, s).await,
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
    context: &crate::operations::OperationContext,
    value: &str,
) -> Result<GraphDefinitionModel, OperationError> {
    if let Some(id) = value
        .strip_prefix("GraphDefinition/")
        .or_else(|| (!value.contains(['/', ':', '|'])).then_some(value))
    {
        return context
            .read(state, "GraphDefinition", id)
            .await?
            .map(|stored| parse_graph_definition(&stored.resource))
            .transpose()?
            .ok_or_else(|| OperationError::NotFound("GraphDefinition not found".into()));
    }

    let (canonical, version) = value
        .split_once('|')
        .map_or((value, None), |(url, version)| (url, Some(version)));
    url::Url::parse(canonical).map_err(|_| {
        OperationError::InvalidParameters(
            "graph requires a GraphDefinition ID or absolute canonical URI".into(),
        )
    })?;
    let mut search_params = SearchParams::new()
        .with_count(2)
        .with_param("url", canonical);
    if let Some(version) = version {
        if version.is_empty() || version.contains('|') {
            return Err(OperationError::InvalidParameters(
                "Invalid graph canonical version".into(),
            ));
        }
        search_params = search_params.with_param("version", version);
    }
    let result = context
        .search(state, "GraphDefinition", &search_params)
        .await?;
    if result.entries.len() > 1 || result.has_more {
        return Err(OperationError::InvalidParameters(
            "GraphDefinition canonical matches multiple resources; supply an unambiguous version"
                .into(),
        ));
    }
    if let Some(entry) = result.entries.first() {
        return parse_graph_definition(&entry.resource);
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

/// A work item for BFS traversal.
struct WorkItem {
    resource: StoredResource,
    links: Vec<LinkModel>,
    depth: usize,
}

/// Traverse the resource graph starting from the focal resource.
async fn traverse(
    state: &AppState,
    context: &crate::operations::OperationContext,
    focal: StoredResource,
    graph_def: &GraphDefinitionModel,
) -> Result<Vec<StoredResource>, OperationError> {
    let mut visited: HashSet<(String, String, String)> = HashSet::new();
    let mut visited_links = HashSet::new();
    let mut results: Vec<StoredResource> = Vec::new();
    let mut queue: VecDeque<WorkItem> = VecDeque::new();
    let mut remapper = crate::bundle_references::BundleReferenceRemapper::default();
    let limits = state.config.search.include_limits;
    let mut bytes = serde_json::to_vec(&focal.resource)
        .map_err(|error| OperationError::Internal(error.to_string()))?
        .len();
    if limits.max_resources == 0 || bytes > limits.max_bytes {
        return Err(OperationError::InvalidParameters(
            "$graph exceeds the configured resource or byte budget".into(),
        ));
    }

    // Seed with focal resource
    visited.insert((
        focal.resource_type.clone(),
        focal.id.clone(),
        focal.version_id.clone(),
    ));
    results.push(focal.clone());
    queue.push_back(WorkItem {
        resource: focal,
        links: graph_def.links.clone(),
        depth: 0,
    });

    while let Some(work) = queue.pop_front() {
        for link in &work.links {
            if !visited_links.insert((
                work.resource.resource_type.clone(),
                work.resource.id.clone(),
                work.resource.version_id.clone(),
                link.clone(),
            )) {
                continue;
            }
            if work.depth >= limits.max_depth {
                return Err(OperationError::InvalidParameters(
                    "$graph traversal exceeds the depth budget".into(),
                ));
            }
            let mut linked = HashSet::new();
            for target in &link.targets {
                let resolved = if let Some(ref path) = link.path {
                    resolve_forward(state, context, &work.resource, path, target, &mut remapper)
                        .await?
                } else if let Some(ref params) = target.params {
                    resolve_reverse(
                        state,
                        context,
                        &work.resource,
                        &target.resource_type,
                        params,
                    )
                    .await?
                } else {
                    continue;
                };

                for res in resolved {
                    if !matches_profile(state, &res.resource, target.profile.as_deref()).await? {
                        continue;
                    }
                    let key = (
                        res.resource_type.clone(),
                        res.id.clone(),
                        res.version_id.clone(),
                    );
                    linked.insert(key.clone());
                    if visited.insert(key) {
                        bytes = bytes.saturating_add(
                            serde_json::to_vec(&res.resource)
                                .map_err(|error| OperationError::Internal(error.to_string()))?
                                .len(),
                        );
                        if results.len() >= limits.max_resources || bytes > limits.max_bytes {
                            return Err(OperationError::InvalidParameters(
                                "$graph traversal exceeds the resource budget".into(),
                            ));
                        }
                        results.push(res.clone());
                    }
                    if !target.links.is_empty() {
                        queue.push_back(WorkItem {
                            resource: res,
                            links: target.links.clone(),
                            depth: work.depth + 1,
                        });
                    }
                }
            }
            if link.min.is_some_and(|min| linked.len() < min)
                || link.max.is_some_and(|max| linked.len() > max)
            {
                return Err(OperationError::InvalidParameters(
                    "Authorized graph does not satisfy link cardinality".into(),
                ));
            }
        }
    }

    Ok(results)
}

/// Forward reference resolution: extract refs from the resource at the path.
async fn resolve_forward(
    state: &AppState,
    context: &crate::operations::OperationContext,
    resource: &StoredResource,
    path: &str,
    target: &TargetModel,
    remapper: &mut crate::bundle_references::BundleReferenceRemapper,
) -> Result<Vec<StoredResource>, OperationError> {
    let refs = link_references(state, &resource.resource, path, remapper).await?;

    let mut results = Vec::new();
    let mut current_ids = Vec::new();
    let mut seen = HashSet::new();
    for fhir_ref in refs {
        if fhir_ref.resource_type != target.resource_type
            || !seen.insert((fhir_ref.id.clone(), fhir_ref.version.clone()))
        {
            continue;
        }
        let Some(version) = &fhir_ref.version else {
            current_ids.push(fhir_ref.id);
            continue;
        };
        let stored = context
            .vread_reference(state, &fhir_ref.resource_type, &fhir_ref.id, version)
            .await;
        match stored {
            Ok(Some(stored)) => results.push(stored),
            Ok(None) => {
                tracing::debug!(
                    reference = %fhir_ref,
                    "Referenced resource not found during $graph traversal"
                );
            }
            Err(e) => {
                if !e.is_forbidden() {
                    return Err(e);
                }
                tracing::debug!(
                    reference = %fhir_ref,
                    error = %e,
                    "Error reading resource during $graph traversal"
                );
            }
        }
    }
    for ids in current_ids.chunks(1000) {
        match context.read_many(state, &target.resource_type, ids).await {
            Ok(resources) => results.extend(resources),
            Err(error) if error.is_forbidden() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(results)
}

/// Reverse reference resolution: search for resources that reference the current one.
async fn resolve_reverse(
    state: &AppState,
    context: &crate::operations::OperationContext,
    resource: &StoredResource,
    target_type: &str,
    params: &str,
) -> Result<Vec<StoredResource>, OperationError> {
    let reference_value = format!("{}/{}", resource.resource_type, resource.id);

    let mut search_params = if params.contains('=') {
        let encoded: String =
            url::form_urlencoded::byte_serialize(reference_value.as_bytes()).collect();
        let query = params.replace("{ref}", &encoded);
        octofhir_search::parse_query_string(&query, 1000, 1000)
    } else {
        SearchParams::new()
            .with_count(1000)
            .with_param(params, &reference_value)
    };

    let mut resources = Vec::new();
    loop {
        match context.search(state, target_type, &search_params).await {
            Ok(result) => {
                let count = result.entries.len();
                resources.extend(result.entries);
                if resources.len() > state.config.search.include_limits.max_resources {
                    return Err(OperationError::InvalidParameters(
                        "$graph reverse traversal exceeds the resource budget".into(),
                    ));
                }
                if !result.has_more {
                    return Ok(resources);
                }
                if count == 0 {
                    return Err(OperationError::Internal(
                        "$graph reverse search made no progress".into(),
                    ));
                }
                search_params.offset = Some(
                    search_params
                        .offset
                        .unwrap_or(0)
                        .checked_add(u32::try_from(count).map_err(|_| {
                            OperationError::InvalidParameters(
                                "$graph search offset overflow".into(),
                            )
                        })?)
                        .ok_or_else(|| {
                            OperationError::InvalidParameters(
                                "$graph search offset overflow".into(),
                            )
                        })?,
                );
            }
            Err(e) => {
                if !e.is_forbidden() {
                    return Err(e);
                }
                tracing::debug!(
                    target_type,
                    params,
                    error = %e,
                    "Error searching reverse references during $graph traversal"
                );
                return Ok(Vec::new());
            }
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
            full_url: Some(format!(
                "{}/fhir/{}/{}",
                base_url.trim_end_matches('/'),
                stored.resource_type,
                stored.id
            )),
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
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance
    }

    fn code(&self) -> &str {
        "graph"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // 1. Resolve GraphDefinition
        let graph_def = resolve_graph_definition(state, context, params).await?;

        // 2. Validate resource type matches GraphDefinition.start
        if graph_def.start != resource_type {
            return Err(OperationError::InvalidParameters(format!(
                "Resource type '{}' does not match GraphDefinition start type '{}'",
                resource_type, graph_def.start
            )));
        }

        // 3. Read the focal resource
        let focal = context
            .read(state, resource_type, id)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;
        if !matches_profile(state, &focal.resource, graph_def.profile.as_deref()).await? {
            return Err(OperationError::InvalidParameters(
                "Focal resource does not satisfy GraphDefinition.profile".into(),
            ));
        }

        // 4. Traverse the graph
        let resources = traverse(state, context, focal, &graph_def).await?;

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

    // -- collect_references_at_path tests --

    #[test]
    fn test_extract_single_reference() {
        let resource = json!({
            "resourceType": "Patient",
            "managingOrganization": {"reference": "Organization/1"}
        });
        let refs = collect_references_at_path(
            &resource,
            "Patient.managingOrganization",
            "http://localhost",
        );
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
            collect_references_at_path(&resource, "DiagnosticReport.result", "http://localhost");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].resource_type, "Observation");
        assert_eq!(refs[1].id, "2");
    }

    #[test]
    fn test_extract_strips_resource_type_prefix() {
        let resource = json!({
            "subject": {"reference": "Patient/123"}
        });
        let refs = collect_references_at_path(&resource, "Observation.subject", "http://localhost");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].resource_type, "Patient");
    }

    #[test]
    fn test_extract_skips_unresolvable_references() {
        let resource = json!({
            "subject": {"reference": "#contained-ref"}
        });
        let refs = collect_references_at_path(&resource, "Observation.subject", "http://localhost");
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
                    "params": "patient={ref}"
                }]
            }]
        });
        let model = parse_graph_definition(&gd).unwrap();
        assert!(model.links[0].path.is_none());
        assert_eq!(
            model.links[0].targets[0].params.as_deref(),
            Some("patient={ref}")
        );
    }

    #[test]
    fn test_parse_missing_start() {
        let gd = json!({"resourceType": "GraphDefinition"});
        assert!(parse_graph_definition(&gd).is_err());
    }

    #[test]
    fn malformed_graph_rules_are_never_dropped() {
        for link in [
            json!({"target":[{}]}),
            json!({"target":[{"type":"Observation","params":"status=final"}]}),
            json!({"path":"Patient.link.other.resolve()","target":[{"type":"Patient"}]}),
            json!({"path":"Patient.link.where(other.resolve().active).other","target":[{"type":"Patient"}]}),
            json!({"path":"Patient.link.other","min":2,"max":"1"}),
            json!({"path":"Patient.link.other","min":-1}),
            json!({"path":"Patient.link.other","max":"invalid"}),
            json!({"path":"Patient.link.other","target":"Patient"}),
            json!({"path":"Patient.link.other","target":[{"type":"Patient","link":[{"target":[{}]}]}]}),
        ] {
            assert!(parse_graph_definition(&json!({"start":"Patient","link":[link]})).is_err());
        }
        assert!(parse_graph_definition(&json!({"start":"Patient","link":{}})).is_err());
        assert!(parse_graph_definition(&json!({"start":"Patient","link":[{"path":"Patient.link.where(other.display = 'resolve()').other","min":0,"max":"*","target":[{"type":"Patient"}]}]})).is_ok());
        for link in [
            json!({"path":"Patient.link.other","sliceName":"linked"}),
            json!({"path":"Patient.link.other","target":[{"type":"Patient","compartment":[{"use":"requirement","code":"Patient","rule":"identical"}]}]}),
        ] {
            assert!(matches!(
                parse_graph_definition(&json!({"start":"Patient","link":[link]})),
                Err(OperationError::NotImplemented(_))
            ));
        }
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
