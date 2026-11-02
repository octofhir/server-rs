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
//!
//! The membership is computed by the strict expander of `octofhir_sof`
//! (the one SQL on FHIR terminology relations use): the ValueSet's own
//! expansion when it carries a complete one, else its `compose` over the
//! server's and the installed packages' code systems and value sets. A part
//! of the definition it cannot evaluate is reported (422), never left out
//! of the expansion. A published ValueSet (installed package, or not found
//! here) beyond it is expanded by the configured terminology server, by
//! canonical; a ValueSet supplied in the request or stored on the server is
//! never sent anywhere. `activeOnly`, `filter`, `offset` / `count` and
//! `includeDesignations` then shape the result.

use async_trait::async_trait;
use octofhir_sof::expansion::{Expander, ExpansionError, Lookup};
use octofhir_sof::plan::ContextArtifacts;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::canonical::get_manager;
use crate::operations::handler::OperationLevel;
use crate::operations::sof::{
    Remote, ServerTerminology, expansion_stamp, packaged, remote_value_set, stored,
};
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Maximum number of codes to return in a single page
const MAX_EXPANSION_COUNT: usize = 10_000;
/// Maximum offset value for pagination
const MAX_OFFSET: usize = 1_000_000;

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

    /// Leave inactive codes out
    #[serde(default)]
    pub active_only: bool,

    /// Exclude nested codes
    #[serde(default)]
    pub exclude_nested: bool,
}

/// The $expand operation handler.
pub struct ExpandOperation;

impl ExpandOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<(ExpandParams, Option<Value>), OperationError> {
        let params = super::lenient_parameters(params);
        let params: &Value = &params;
        // Check if this is a Parameters resource
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut expand_params = ExpandParams::default();
            let mut inline_value_set: Option<Value> = None;

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "url" => {
                            expand_params.url = param
                                .get("valueUri")
                                .and_then(|v| v.as_str())
                                .map(String::from);
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
                                .map(|v| v.max(0) as usize) // Clamp negative to 0
                                .map(|v| v.min(MAX_OFFSET)) // Cap at maximum
                                .unwrap_or(0);
                        }
                        "count" => {
                            expand_params.count = param
                                .get("valueInteger")
                                .and_then(|v| v.as_i64())
                                .map(|v| v.max(1) as usize) // At least 1
                                .map(|v| v.min(MAX_EXPANSION_COUNT)); // Cap at maximum
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
                                .unwrap_or(false);
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

    /// The ValueSet `url` (`version`) names: stored on the server (as the
    /// caller may read it), else from an installed package, else expanded by
    /// the terminology server. The flag says whether it is published (may be
    /// expanded remotely).
    async fn resolve_value_set_by_url(
        state: &AppState,
        context: &OperationContext,
        url: &str,
        version: Option<&str>,
    ) -> Result<(Value, bool), OperationError> {
        let named = match version {
            Some(version) => format!("{url}|{version}"),
            None => url.to_owned(),
        };
        match stored(state, context, "ValueSet", url, version).await? {
            Lookup::Found(resolved) => return Ok((resolved.resource, false)),
            Lookup::Ambiguous(reason) => return Err(OperationError::NotFound(reason)),
            Lookup::Missing => {}
        }
        match packaged("ValueSet", url, version).await? {
            Lookup::Found(resolved) => return Ok((resolved.resource, true)),
            Lookup::Ambiguous(reason) => return Err(OperationError::NotFound(reason)),
            Lookup::Missing => {}
        }
        let server = state.config.terminology.server_url.trim_end_matches('/');
        if !server.is_empty()
            && let Ok(Remote::Expanded(value_set)) = remote_value_set(server, url, version).await
        {
            return Ok((value_set, false));
        }
        Err(OperationError::NotFound(format!(
            "ValueSet with url '{named}' not found"
        )))
    }

    /// Load a ValueSet by ID from storage.
    async fn load_value_set_by_id(
        state: &AppState,
        id: &str,
    ) -> Result<(Value, bool), OperationError> {
        let result = state
            .storage
            .read("ValueSet", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to read ValueSet: {}", e)))?;

        match result {
            Some(stored) => Ok((stored.resource, false)),
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
                        .map(|r| (r.resource.content, true))
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

    /// Expand `value_set` and shape the result by `params`. `published`:
    /// the ValueSet comes from an installed package, so the terminology
    /// server may expand what the local expander cannot.
    async fn expand_value_set(
        state: &AppState,
        context: &OperationContext,
        value_set: &Value,
        published: bool,
        params: &ExpandParams,
    ) -> Result<Value, OperationError> {
        let inline = ContextArtifacts::default();
        let source = ServerTerminology {
            state,
            context,
            inline: &inline,
        };
        let expanded = Expander::new(&source)
            .with_designations(params.include_designations)
            .expand(value_set, &expansion_stamp())
            .await;
        let expansion = match expanded {
            Ok(expansion) => expansion,
            Err(ExpansionError::Undeterminable(reason)) => {
                let server = state.config.terminology.server_url.trim_end_matches('/');
                let url = value_set["url"].as_str();
                match (published && !server.is_empty(), url) {
                    (true, Some(url)) => {
                        match remote_value_set(server, url, value_set["version"].as_str()).await {
                            Ok(Remote::Expanded(remote)) => remote["expansion"].clone(),
                            _ => return Err(cannot_expand(value_set, &reason)),
                        }
                    }
                    _ => return Err(cannot_expand(value_set, &reason)),
                }
            }
            Err(ExpansionError::Source(error)) => {
                return Err(match error.downcast::<OperationError>() {
                    Ok(error) => *error,
                    Err(error) => OperationError::Internal(error.to_string()),
                });
            }
        };
        Ok(shape(value_set, &expansion, params))
    }
}

/// A 422 naming what keeps `value_set` from being expanded.
fn cannot_expand(value_set: &Value, reason: &str) -> OperationError {
    let named = value_set["url"].as_str().unwrap_or("(inline)");
    OperationError::Api(octofhir_api::ApiError::unprocessable_entity(
        format!("ValueSet {named} cannot be expanded: {reason}"),
        None,
    ))
}

/// The response: `value_set` with the page of `expansion` the parameters
/// ask for (flattened; inactive codes dropped under `activeOnly`; text
/// `filter` on code and display).
fn shape(value_set: &Value, expansion: &Value, params: &ExpandParams) -> Value {
    let mut entries: Vec<Value> = Vec::new();
    let mut pending: Vec<&Value> = vec![&expansion["contains"]];
    while let Some(list) = pending.pop() {
        for entry in list.as_array().into_iter().flatten().rev() {
            pending.push(&entry["contains"]);
        }
        // Children were pushed after; keep document order for this level.
        let level: Vec<Value> = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| {
                let mut flat = entry.clone();
                if let Some(object) = flat.as_object_mut() {
                    object.remove("contains");
                    if !params.include_designations {
                        object.remove("designation");
                    }
                }
                flat
            })
            .collect();
        entries.extend(level);
    }
    if params.active_only {
        entries.retain(|entry| entry["inactive"] != true);
    }
    if let Some(filter) = &params.filter {
        let filter = filter.to_lowercase();
        entries.retain(|entry| {
            ["code", "display"].iter().any(|field| {
                entry[*field]
                    .as_str()
                    .is_some_and(|text| text.to_lowercase().contains(&filter))
            })
        });
    }
    let total = entries.len();
    let offset = params.offset.min(MAX_OFFSET);
    let count = params
        .count
        .unwrap_or(MAX_EXPANSION_COUNT)
        .min(MAX_EXPANSION_COUNT);
    let page: Vec<Value> = entries.into_iter().skip(offset).take(count).collect();

    let mut parameters: Vec<Value> = expansion["parameter"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(filter) = &params.filter {
        parameters.push(json!({"name": "filter", "valueString": filter}));
    }
    if params.offset > 0 {
        parameters.push(json!({"name": "offset", "valueInteger": params.offset}));
    }
    if let Some(count) = params.count {
        parameters.push(json!({"name": "count", "valueInteger": count}));
    }
    if params.include_designations {
        parameters.push(json!({"name": "includeDesignations", "valueBoolean": true}));
    }
    if params.active_only {
        parameters.push(json!({"name": "activeOnly", "valueBoolean": true}));
    }
    let mut shaped = json!({
        "identifier": expansion["identifier"],
        "timestamp": expansion["timestamp"],
        "total": total,
        "offset": offset,
        "contains": page,
    });
    if !parameters.is_empty() {
        shaped["parameter"] = Value::Array(parameters);
    }
    let mut response = value_set.clone();
    response["expansion"] = shaped;
    response
}

impl Default for ExpandOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for ExpandOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::System || resource_type == Some("ValueSet")
    }

    fn code(&self) -> &str {
        "expand"
    }

    /// Handle system-level $expand (POST /$expand).
    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let (expand_params, inline_value_set) = Self::extract_params(params)?;
        let (value_set, published) = match inline_value_set {
            Some(value_set) => (value_set, false),
            None => match expand_params.url.as_deref() {
                Some(url) => {
                    Self::resolve_value_set_by_url(
                        state,
                        context,
                        url,
                        expand_params.value_set_version.as_deref(),
                    )
                    .await?
                }
                None => {
                    return Err(OperationError::InvalidParameters(
                        "Either 'url' or 'valueSet' parameter is required".into(),
                    ));
                }
            },
        };
        Self::expand_value_set(state, context, &value_set, published, &expand_params).await
    }

    /// Handle type-level $expand (GET/POST /ValueSet/$expand).
    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "ValueSet" {
            return Err(OperationError::NotSupported(format!(
                "$expand is only supported on ValueSet, not {}",
                resource_type
            )));
        }
        self.handle_system(state, context, params).await
    }

    /// Handle instance-level $expand (GET/POST /ValueSet/{id}/$expand).
    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
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
        let (value_set, published) = Self::load_value_set_by_id(state, id).await?;
        Self::expand_value_set(state, context, &value_set, published, &expand_params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansions_are_flattened_filtered_and_paged() {
        let value_set = json!({"resourceType": "ValueSet", "url": "http://x/vs"});
        let expansion = json!({"identifier": "urn:uuid:1", "timestamp": "2026-11-02T00:00:00Z",
        "parameter": [{"name": "version", "valueUri": "http://x/cs|1"}],
        "contains": [
            {"system": "http://x/cs", "code": "a", "display": "Alpha", "contains": [
                {"system": "http://x/cs", "code": "b", "display": "Beta", "inactive": true}]},
            {"system": "http://x/cs", "code": "c", "display": "Gamma",
             "designation": [{"value": "g"}]}
        ]});
        let all = shape(&value_set, &expansion, &ExpandParams::default());
        let codes: Vec<&str> = all["expansion"]["contains"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, ["a", "c", "b"]);
        assert_eq!(all["expansion"]["total"], 3);
        assert!(all["expansion"]["contains"][1].get("designation").is_none());

        let params = ExpandParams {
            active_only: true,
            filter: Some("A".into()),
            count: Some(1),
            offset: 1,
            ..Default::default()
        };
        let page = shape(&value_set, &expansion, &params);
        assert_eq!(page["expansion"]["total"], 2);
        assert_eq!(page["expansion"]["offset"], 1);
        assert_eq!(page["expansion"]["contains"][0]["code"], "c");
        assert_eq!(page["expansion"]["parameter"][0]["name"], "version");
    }

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
}
