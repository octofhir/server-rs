//! $meta, $meta-add, and $meta-delete operation handlers.
//!
//! This module implements the FHIR meta operations for retrieving and
//! managing resource metadata (profiles, tags, security labels).

use async_trait::async_trait;
use octofhir_api::ApiError;
use octofhir_core::ResourceType;
use serde_json::{Value, json};
use std::str::FromStr;

use super::{OperationError, OperationHandler};
use crate::server::AppState;

/// Extracts the meta parameter from a Parameters resource.
fn extract_meta_parameter(params: &Value) -> Result<Value, OperationError> {
    if params["resourceType"] != "Parameters" {
        return Err(OperationError::InvalidParameters(
            "Expected a Parameters resource".into(),
        ));
    }
    let parameters = params["parameter"]
        .as_array()
        .ok_or_else(|| OperationError::InvalidParameters("meta parameter required".into()))?;
    let mut matches = parameters.iter().filter(|p| p["name"] == "meta");
    let parameter = matches
        .next()
        .ok_or_else(|| OperationError::InvalidParameters("meta parameter required".into()))?;
    if matches.next().is_some() {
        return Err(OperationError::InvalidParameters(
            "meta permits exactly one value".into(),
        ));
    }
    if parameter.as_object().is_some_and(|object| {
        object.keys().any(|key| {
            key == "resource" || key == "part" || (key.starts_with("value") && key != "valueMeta")
        })
    }) {
        return Err(OperationError::InvalidParameters(
            "meta requires a single valueMeta".into(),
        ));
    }
    parameter["valueMeta"]
        .as_object()
        .map(|object| Value::Object(object.clone()))
        .ok_or_else(|| OperationError::InvalidParameters("meta requires valueMeta".into()))
}

/// Check the supplied datatype and resulting profile claims without secondary
/// resource reads. Metadata edits do not change references and require update
/// permission independently of read. Never return diagnostics derived from the
/// existing clinical body to a caller who may only have update permission.
pub(super) async fn validate_metadata_update(
    state: &AppState,
    metadata: &Value,
    resource: &Value,
) -> Result<(), OperationError> {
    let validation = crate::validation::ValidationService::with_options(
        state.model_provider.clone(),
        state.fhirpath_engine.clone(),
        None,
        state.terminology_provider.clone().map(|provider| {
            std::sync::Arc::new(octofhir_fhirschema::TerminologyProviderAdapter::new(
                provider,
            ))
                as std::sync::Arc<dyn octofhir_fhirschema::terminology::TerminologyService>
        }),
    );
    let parameters = json!({"resourceType":"Parameters","parameter":[{
        "name":"meta", "valueMeta":metadata
    }]});
    if !validation.validate(&parameters).await.valid {
        return Err(OperationError::InvalidParameters(
            "Invalid valueMeta datatype".into(),
        ));
    }
    if !validation.validate(resource).await.valid {
        return Err(ApiError::unprocessable_entity(
            "Metadata update does not conform to the resource schema or available declared profiles",
            None,
        ).into());
    }
    Ok(())
}

/// FHIR JSON omits empty arrays, including a set emptied by $meta-delete.
fn set_meta_values(meta: &mut Value, key: &str, values: Vec<Value>) {
    if values.is_empty() {
        if let Some(object) = meta.as_object_mut() {
            object.remove(key);
        }
    } else {
        meta[key] = json!(values);
    }
}

/// A repeating primitive has parallel value and id/extension arrays. Work on
/// pairs so deduplication and removal cannot attach extensions to another URL.
fn profile_entries(meta: &Value) -> Vec<(Value, Value)> {
    let values = meta["profile"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let extensions = meta["_profile"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    (0..values.len().max(extensions.len()))
        .map(|index| {
            (
                values.get(index).cloned().unwrap_or(Value::Null),
                extensions.get(index).cloned().unwrap_or(Value::Null),
            )
        })
        .collect()
}

fn same_profile(left: &(Value, Value), right: &(Value, Value)) -> bool {
    if left.0.is_null() || right.0.is_null() {
        // Extension-only elements have no canonical URL to identify them.
        // Preserve distinct elements rather than conflating every null slot.
        left == right
    } else {
        left.0 == right.0
    }
}

fn set_profile_entries(meta: &mut Value, entries: Vec<(Value, Value)>) {
    let (mut values, mut extensions): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
    if values.iter().all(Value::is_null) {
        values.clear();
    }
    if extensions.iter().all(Value::is_null) {
        extensions.clear();
    }
    set_meta_values(meta, "profile", values);
    set_meta_values(meta, "_profile", extensions);
}

/// Merges meta elements from meta_to_add into the resource's meta field.
fn merge_meta_into_resource(resource: &mut Value, meta_to_add: &Value) {
    // Ensure meta exists
    if !resource["meta"].is_object() {
        resource["meta"] = json!({});
    }
    let meta = resource.get_mut("meta").unwrap();

    // Merge profiles (avoid duplicates)
    if meta_to_add.get("profile").is_some() || meta_to_add.get("_profile").is_some() {
        let mut new_profiles = profile_entries(meta);
        for profile in profile_entries(meta_to_add) {
            if !new_profiles
                .iter()
                .any(|existing| same_profile(existing, &profile))
            {
                new_profiles.push(profile);
            }
        }
        set_profile_entries(meta, new_profiles);
    }

    // Merge tags (match by system+code)
    if let Some(tags_to_add) = meta_to_add["tag"].as_array() {
        let mut new_tags = meta["tag"].as_array().cloned().unwrap_or_default();
        for tag in tags_to_add {
            let already_exists = new_tags
                .iter()
                .any(|t| t["system"] == tag["system"] && t["code"] == tag["code"]);
            if !already_exists {
                new_tags.push(tag.clone());
            }
        }
        set_meta_values(meta, "tag", new_tags);
    }

    // Merge security labels (match by system+code)
    if let Some(security_to_add) = meta_to_add["security"].as_array() {
        let mut new_security = meta["security"].as_array().cloned().unwrap_or_default();
        for label in security_to_add {
            let already_exists = new_security
                .iter()
                .any(|s| s["system"] == label["system"] && s["code"] == label["code"]);
            if !already_exists {
                new_security.push(label.clone());
            }
        }
        set_meta_values(meta, "security", new_security);
    }
}

/// Removes meta elements from the resource's meta field.
fn remove_meta_from_resource(resource: &mut Value, meta_to_delete: &Value) {
    if !resource["meta"].is_object() {
        return;
    }
    let meta = resource.get_mut("meta").unwrap();

    // Remove profiles
    if meta_to_delete.get("profile").is_some() || meta_to_delete.get("_profile").is_some() {
        let profiles_to_remove = profile_entries(meta_to_delete);
        let filtered = profile_entries(meta)
            .into_iter()
            .filter(|profile| {
                !profiles_to_remove
                    .iter()
                    .any(|removed| same_profile(profile, removed))
            })
            .collect();
        set_profile_entries(meta, filtered);
    }

    // Remove tags (match by system+code)
    if let Some(tags_to_remove) = meta_to_delete["tag"].as_array()
        && let Some(existing) = meta["tag"].as_array()
    {
        let filtered: Vec<Value> = existing
            .iter()
            .filter(|t| {
                !tags_to_remove
                    .iter()
                    .any(|tr| t["system"] == tr["system"] && t["code"] == tr["code"])
            })
            .cloned()
            .collect();
        set_meta_values(meta, "tag", filtered);
    }

    // Remove security labels (match by system+code)
    if let Some(security_to_remove) = meta_to_delete["security"].as_array()
        && let Some(existing) = meta["security"].as_array()
    {
        let filtered: Vec<Value> = existing
            .iter()
            .filter(|s| {
                !security_to_remove
                    .iter()
                    .any(|sr| s["system"] == sr["system"] && s["code"] == sr["code"])
            })
            .cloned()
            .collect();
        set_meta_values(meta, "security", filtered);
    }
}

/// Extracts meta from a resource and builds a clean meta Value for the response.
fn extract_meta_from_resource(resource: &Value) -> Value {
    resource.get("meta").cloned().unwrap_or_else(|| json!({}))
}

/// Builds a Parameters response containing a meta value.
fn build_meta_response(meta: Value) -> Value {
    json!({
        "resourceType": "Parameters",
        "parameter": [{
            "name": "return",
            "valueMeta": meta
        }]
    })
}

/// Parse resource type string into ResourceType enum.
fn parse_resource_type(resource_type: &str) -> Result<ResourceType, OperationError> {
    ResourceType::from_str(resource_type).map_err(|_| {
        OperationError::InvalidParameters(format!("Invalid resource type: {}", resource_type))
    })
}

// ============================================================================
// $meta Operation
// ============================================================================

/// The $meta operation handler.
///
/// Returns the metadata (profiles, tags, security labels) for a resource.
pub struct MetaOperation;

#[async_trait]
impl OperationHandler for MetaOperation {
    fn code(&self) -> &str {
        "meta"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        // System-level meta aggregation is expensive and not commonly needed
        Err(OperationError::NotSupported(
            "Operation $meta is not supported at system level".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        // Type-level meta aggregation would require querying all resources
        // This is expensive and not implemented
        Err(OperationError::NotSupported(
            "Operation $meta is not supported at type level".into(),
        ))
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Fetch the resource from storage using modern API
        let stored = context
            .read(state, resource_type, id)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Extract and return meta
        let meta_json = extract_meta_from_resource(&stored.resource);
        Ok(build_meta_response(meta_json))
    }
}

// ============================================================================
// $meta-add Operation
// ============================================================================

/// The $meta-add operation handler.
///
/// Adds profiles, tags, and/or security labels to a resource's metadata.
pub struct MetaAddOperation;

#[async_trait]
impl OperationHandler for MetaAddOperation {
    fn code(&self) -> &str {
        "meta-add"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-add is not supported at system level".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-add is not supported at type level".into(),
        ))
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Extract meta to add from parameters
        let meta_to_add = extract_meta_parameter(params)?;

        // Get current resource from storage using modern API
        let stored = context
            .read_for_update(state, resource_type, id)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Clone and modify the resource
        let mut resource = stored.resource.clone();
        merge_meta_into_resource(&mut resource, &meta_to_add);

        // Update the resource in storage
        let updated = context
            .update_metadata(state, &stored, &resource, &meta_to_add)
            .await?;

        // Return the updated meta
        let meta_json = extract_meta_from_resource(&updated.resource);
        Ok(build_meta_response(meta_json))
    }
}

// ============================================================================
// $meta-delete Operation
// ============================================================================

/// The $meta-delete operation handler.
///
/// Removes profiles, tags, and/or security labels from a resource's metadata.
pub struct MetaDeleteOperation;

#[async_trait]
impl OperationHandler for MetaDeleteOperation {
    fn code(&self) -> &str {
        "meta-delete"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-delete is not supported at system level".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-delete is not supported at type level".into(),
        ))
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Extract meta to delete from parameters
        let meta_to_delete = extract_meta_parameter(params)?;

        // Get current resource from storage using modern API
        let stored = context
            .read_for_update(state, resource_type, id)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Clone and modify the resource
        let mut resource = stored.resource.clone();
        remove_meta_from_resource(&mut resource, &meta_to_delete);

        // Update the resource in storage
        let updated = context
            .update_metadata(state, &stored, &resource, &meta_to_delete)
            .await?;

        // Return the updated meta
        let meta_json = extract_meta_from_resource(&updated.resource);
        Ok(build_meta_response(meta_json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_meta_parameter() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "meta",
                "valueMeta": {
                    "tag": [{"system": "http://example.org", "code": "test"}]
                }
            }]
        });

        let meta = extract_meta_parameter(&params).unwrap();
        assert!(meta["tag"].is_array());
        assert_eq!(meta["tag"][0]["code"], "test");
    }

    #[test]
    fn test_extract_meta_parameter_missing() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": []
        });

        let result = extract_meta_parameter(&params);
        assert!(result.is_err());
    }

    #[test]
    fn meta_requires_one_parameter_with_one_typed_value() {
        let meta = json!({"name":"meta", "valueMeta":{"profile":["urn:test:profile"]}});
        for params in [
            json!({"resourceType":"Patient", "parameter":[meta.clone()]}),
            json!({"resourceType":"Parameters", "parameter":[meta.clone(), meta]}),
            json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueString":"bad"}]}),
            json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":{}, "valueString":"bad"}]}),
            json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":{}, "resource":{"resourceType":"Patient"}}]}),
            json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":{}, "part":[]}]}),
            json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":[]}]}),
        ] {
            assert!(extract_meta_parameter(&params).is_err(), "{params}");
        }
    }

    #[test]
    fn metadata_sets_deduplicate_within_request_and_ignore_coding_display_and_version() {
        let mut resource = json!({"resourceType":"Patient", "meta":{
            "tag":[{"system":"urn:test", "code":"existing", "display":"original"}]
        }});
        merge_meta_into_resource(
            &mut resource,
            &json!({
                "profile":["urn:test:profile", "urn:test:profile"],
                "tag":[
                    {"system":"urn:test", "code":"existing", "display":"replacement"},
                    {"system":"urn:test", "code":"new", "version":"1"},
                    {"system":"urn:test", "code":"new", "version":"2"}
                ],
                "security":[
                    {"system":"urn:test", "code":"label", "display":"first"},
                    {"system":"urn:test", "code":"label", "display":"second"}
                ]
            }),
        );
        assert_eq!(resource["meta"]["profile"], json!(["urn:test:profile"]));
        assert_eq!(resource["meta"]["tag"].as_array().unwrap().len(), 2);
        assert_eq!(resource["meta"]["tag"][0]["display"], "original");
        assert_eq!(resource["meta"]["tag"][1]["version"], "1");
        assert_eq!(resource["meta"]["security"].as_array().unwrap().len(), 1);

        remove_meta_from_resource(
            &mut resource,
            &json!({
                "profile":["urn:test:profile", "urn:test:absent"],
                "tag":[
                    {"system":"urn:test", "code":"existing", "display":"irrelevant"},
                    {"system":"urn:test", "code":"new", "version":"other"}
                ],
                "security":[{"system":"urn:test", "code":"label", "display":"other"}]
            }),
        );
        assert_eq!(resource["meta"], json!({}));
    }

    #[test]
    fn profile_extensions_stay_with_their_url_during_set_operations() {
        let mut resource = json!({"resourceType":"Patient", "meta":{
            "profile":["urn:test:plain", "urn:test:annotated"],
            "_profile":[null, {"id":"original"}]
        }});
        merge_meta_into_resource(
            &mut resource,
            &json!({
                "profile":["urn:test:annotated", "urn:test:new", "urn:test:new", null, null],
                "_profile":[{"id":"ignored"}, {"id":"new"}, {"id":"duplicate"},
                    {"id":"extension-only-a"}, {"id":"extension-only-b"}]
            }),
        );
        assert_eq!(
            resource["meta"]["profile"],
            json!([
                "urn:test:plain",
                "urn:test:annotated",
                "urn:test:new",
                null,
                null
            ])
        );
        assert_eq!(
            resource["meta"]["_profile"],
            json!([
                null, {"id":"original"}, {"id":"new"},
                {"id":"extension-only-a"}, {"id":"extension-only-b"}
            ])
        );
        remove_meta_from_resource(
            &mut resource,
            &json!({
                "profile":["urn:test:plain", "urn:test:new"]
            }),
        );
        assert_eq!(
            resource["meta"]["profile"],
            json!(["urn:test:annotated", null, null])
        );
        assert_eq!(
            resource["meta"]["_profile"],
            json!([
                {"id":"original"}, {"id":"extension-only-a"}, {"id":"extension-only-b"}
            ])
        );
        remove_meta_from_resource(&mut resource, &json!({"profile":["urn:test:annotated"]}));
        assert!(resource["meta"].get("profile").is_none());
        assert_eq!(
            resource["meta"]["_profile"],
            json!([
                {"id":"extension-only-a"}, {"id":"extension-only-b"}
            ])
        );
        remove_meta_from_resource(
            &mut resource,
            &json!({"_profile":[
                {"id":"extension-only-a"}, {"id":"extension-only-b"}
            ]}),
        );
        assert_eq!(resource["meta"], json!({}));
    }

    #[test]
    fn test_build_meta_response() {
        let meta = json!({
            "profile": ["http://example.org/profile"],
            "tag": [{"system": "http://example.org", "code": "test"}]
        });

        let response = build_meta_response(meta);

        assert_eq!(response["resourceType"], "Parameters");
        assert_eq!(response["parameter"][0]["name"], "return");
        assert!(response["parameter"][0]["valueMeta"]["profile"].is_array());
    }

    #[test]
    fn test_operation_codes() {
        assert_eq!(MetaOperation.code(), "meta");
        assert_eq!(MetaAddOperation.code(), "meta-add");
        assert_eq!(MetaDeleteOperation.code(), "meta-delete");
    }
}
