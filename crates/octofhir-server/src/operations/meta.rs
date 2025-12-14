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

/// Merges meta elements from meta_to_add into the resource's meta field.
fn merge_meta_into_resource(resource: &mut Value, meta_to_add: &Value) {
    // Ensure meta exists
    if resource.get("meta").is_none() {
        resource["meta"] = json!({});
    }
    let meta = resource.get_mut("meta").unwrap();

    // Merge profiles (avoid duplicates)
    if let Some(profiles_to_add) = meta_to_add["profile"].as_array() {
        let existing_profiles = meta["profile"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut new_profiles = existing_profiles.clone();
        for profile in profiles_to_add {
            if !existing_profiles.contains(profile) {
                new_profiles.push(profile.clone());
            }
        }
        meta["profile"] = json!(new_profiles);
    }

    // Merge tags (match by system+code)
    if let Some(tags_to_add) = meta_to_add["tag"].as_array() {
        let existing_tags = meta["tag"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut new_tags = existing_tags.clone();
        for tag in tags_to_add {
            let already_exists = existing_tags
                .iter()
                .any(|t| t["system"] == tag["system"] && t["code"] == tag["code"]);
            if !already_exists {
                new_tags.push(tag.clone());
            }
        }
        meta["tag"] = json!(new_tags);
    }

    // Merge security labels (match by system+code)
    if let Some(security_to_add) = meta_to_add["security"].as_array() {
        let existing_security = meta["security"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut new_security = existing_security.clone();
        for label in security_to_add {
            let already_exists = existing_security
                .iter()
                .any(|s| s["system"] == label["system"] && s["code"] == label["code"]);
            if !already_exists {
                new_security.push(label.clone());
            }
        }
        meta["security"] = json!(new_security);
    }
}

/// Removes meta elements from the resource's meta field.
fn remove_meta_from_resource(resource: &mut Value, meta_to_delete: &Value) {
    if resource.get("meta").is_none() {
        return;
    }
    let meta = resource.get_mut("meta").unwrap();

    // Remove profiles
    if let Some(profiles_to_remove) = meta_to_delete["profile"].as_array() {
        if let Some(existing) = meta["profile"].as_array() {
            let filtered: Vec<Value> = existing
                .iter()
                .filter(|p| !profiles_to_remove.contains(p))
                .cloned()
                .collect();
            meta["profile"] = json!(filtered);
        }
    }

    // Remove tags (match by system+code)
    if let Some(tags_to_remove) = meta_to_delete["tag"].as_array() {
        if let Some(existing) = meta["tag"].as_array() {
            let filtered: Vec<Value> = existing
                .iter()
                .filter(|t| {
                    !tags_to_remove
                        .iter()
                        .any(|tr| t["system"] == tr["system"] && t["code"] == tr["code"])
                })
                .cloned()
                .collect();
            meta["tag"] = json!(filtered);
        }
    }

    // Remove security labels (match by system+code)
    if let Some(security_to_remove) = meta_to_delete["security"].as_array() {
        if let Some(existing) = meta["security"].as_array() {
            let filtered: Vec<Value> = existing
                .iter()
                .filter(|s| {
                    !security_to_remove
                        .iter()
                        .any(|sr| s["system"] == sr["system"] && s["code"] == sr["code"])
                })
                .cloned()
                .collect();
            meta["security"] = json!(filtered);
        }
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
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Fetch the resource from storage using modern API
        let stored = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
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
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Extract meta to add from parameters
        let meta_to_add = extract_meta_parameter(params)?;

        // Get current resource from storage using modern API
        let stored = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Clone and modify the resource
        let mut resource = stored.resource.clone();
        merge_meta_into_resource(&mut resource, &meta_to_add);

        // Update the resource in storage
        let updated = state
            .storage
            .update(&resource, None)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

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
        // Validate resource type
        let _rt = parse_resource_type(resource_type)?;

        // Extract meta to delete from parameters
        let meta_to_delete = extract_meta_parameter(params)?;

        // Get current resource from storage using modern API
        let stored = state
            .storage
            .read(resource_type, id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("{}/{} not found", resource_type, id))
            })?;

        // Clone and modify the resource
        let mut resource = stored.resource.clone();
        remove_meta_from_resource(&mut resource, &meta_to_delete);

        // Update the resource in storage
        let updated = state
            .storage
            .update(&resource, None)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

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
