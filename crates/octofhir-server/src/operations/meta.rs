//! $meta, $meta-add, and $meta-delete operation handlers.
//!
//! This module implements the FHIR meta operations for retrieving and
//! managing resource metadata (profiles, tags, security labels).

use async_trait::async_trait;
use octofhir_core::ResourceType;
use serde_json::{Value, json};
use std::str::FromStr;

use super::{OperationError, OperationHandler};
use crate::server::AppState;

/// Extracts the meta parameter from a Parameters resource.
fn extract_meta_parameter(params: &Value) -> Result<Value, OperationError> {
    params["parameter"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|p| p["name"].as_str() == Some("meta"))
                .and_then(|p| p["valueMeta"].as_object())
                .map(|o| Value::Object(o.clone()))
        })
        .ok_or_else(|| OperationError::InvalidParameters("meta parameter required".into()))
}

/// Merges meta elements from meta_to_add into the envelope's meta.
fn merge_meta_into_envelope(envelope: &mut octofhir_core::ResourceEnvelope, meta_to_add: &Value) {
    // Merge profiles (avoid duplicates)
    if let Some(profiles) = meta_to_add["profile"].as_array() {
        for profile in profiles {
            if let Some(p) = profile.as_str()
                && !envelope.meta.profile.contains(&p.to_string())
            {
                envelope.meta.profile.push(p.to_string());
            }
        }
    }

    // Merge tags (match by system+code)
    if let Some(tags) = meta_to_add["tag"].as_array() {
        for tag in tags {
            let already_exists = envelope
                .meta
                .tag
                .iter()
                .any(|t| t["system"] == tag["system"] && t["code"] == tag["code"]);
            if !already_exists {
                envelope.meta.tag.push(tag.clone());
            }
        }
    }

    // Merge security labels (match by system+code)
    if let Some(security) = meta_to_add["security"].as_array() {
        for label in security {
            let already_exists = envelope
                .meta
                .security
                .iter()
                .any(|s| s["system"] == label["system"] && s["code"] == label["code"]);
            if !already_exists {
                envelope.meta.security.push(label.clone());
            }
        }
    }
}

/// Removes meta elements from the envelope's meta.
fn remove_meta_from_envelope(
    envelope: &mut octofhir_core::ResourceEnvelope,
    meta_to_delete: &Value,
) {
    // Remove profiles
    if let Some(profiles_to_remove) = meta_to_delete["profile"].as_array() {
        let profiles_set: Vec<&str> = profiles_to_remove
            .iter()
            .filter_map(|p| p.as_str())
            .collect();
        envelope
            .meta
            .profile
            .retain(|p| !profiles_set.contains(&p.as_str()));
    }

    // Remove tags (match by system+code)
    if let Some(tags_to_remove) = meta_to_delete["tag"].as_array() {
        envelope.meta.tag.retain(|t| {
            !tags_to_remove
                .iter()
                .any(|tr| t["system"] == tr["system"] && t["code"] == tr["code"])
        });
    }

    // Remove security labels (match by system+code)
    if let Some(security_to_remove) = meta_to_delete["security"].as_array() {
        envelope.meta.security.retain(|s| {
            !security_to_remove
                .iter()
                .any(|sr| s["system"] == sr["system"] && s["code"] == sr["code"])
        });
    }
}

/// Converts ResourceMeta to a JSON Value for the response.
fn meta_to_json(meta: &octofhir_core::ResourceMeta) -> Value {
    let mut result = json!({});

    if !meta.profile.is_empty() {
        result["profile"] = json!(meta.profile);
    }

    if !meta.tag.is_empty() {
        result["tag"] = json!(meta.tag);
    }

    if !meta.security.is_empty() {
        result["security"] = json!(meta.security);
    }

    if let Some(ref version_id) = meta.version_id {
        result["versionId"] = json!(version_id);
    }

    result["lastUpdated"] = json!(meta.last_updated.to_string());

    if let Some(ref source) = meta.source {
        result["source"] = json!(source);
    }

    result
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
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        let rt = parse_resource_type(resource_type)?;

        // Fetch the resource from storage
        let envelope = state
            .storage
            .get(&rt, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Convert meta to JSON and return
        let meta_json = meta_to_json(&envelope.meta);
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
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-add is not supported at system level".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
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
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let rt = parse_resource_type(resource_type)?;

        // Extract meta to add from parameters
        let meta_to_add = extract_meta_parameter(params)?;

        // Get current resource from storage
        let mut envelope = state
            .storage
            .get(&rt, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Merge the new meta elements
        merge_meta_into_envelope(&mut envelope, &meta_to_add);

        // Update the resource in storage
        let updated = state
            .storage
            .update(&rt, id, envelope)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        // Return the updated meta
        let meta_json = meta_to_json(&updated.meta);
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
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $meta-delete is not supported at system level".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
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
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let rt = parse_resource_type(resource_type)?;

        // Extract meta to delete from parameters
        let meta_to_delete = extract_meta_parameter(params)?;

        // Get current resource from storage
        let mut envelope = state
            .storage
            .get(&rt, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Remove the meta elements
        remove_meta_from_envelope(&mut envelope, &meta_to_delete);

        // Update the resource in storage
        let updated = state
            .storage
            .update(&rt, id, envelope)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        // Return the updated meta
        let meta_json = meta_to_json(&updated.meta);
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
