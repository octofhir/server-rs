//! JSON Patch (RFC 6902) and FHIRPath Patch implementation for FHIR resources.

use std::sync::Arc;

use json_patch::{Patch, PatchOperation, patch};
use octofhir_api::ApiError;
use octofhir_fhir_model::provider::ModelProvider;
use octofhir_fhirpath::FhirPathEngine;
use serde_json::Value;

/// Type alias for shared model provider
pub type SharedModelProvider = Arc<dyn ModelProvider + Send + Sync>;

mod locations;

// ============================================================================
// JSON Patch (RFC 6902)
// ============================================================================

/// Applies a JSON Patch (RFC 6902) to a FHIR resource.
///
/// # Arguments
/// * `resource` - The current resource JSON to patch
/// * `patch_bytes` - Raw JSON Patch document bytes
///
/// # Returns
/// The patched resource or an error if the patch is invalid or fails to apply
pub fn apply_json_patch(resource: &Value, patch_bytes: &[u8]) -> Result<Value, ApiError> {
    // Parse patch operations
    let operations: Patch = serde_json::from_slice(patch_bytes)
        .map_err(|e| ApiError::bad_request(format!("Invalid JSON Patch document: {e}")))?;

    // Validate patch operations before applying
    validate_json_patch_operations(&operations.0)?;

    // Clone resource for modification
    let mut patched = resource.clone();

    // Apply patch
    patch(&mut patched, &operations)
        .map_err(|e| ApiError::bad_request(format!("Patch operation failed: {e}")))?;

    Ok(patched)
}

/// Validates that JSON Patch operations don't modify protected fields.
fn validate_json_patch_operations(operations: &[PatchOperation]) -> Result<(), ApiError> {
    for op in operations {
        // A test observes identity; it does not change it (RFC 6902).
        if matches!(op, PatchOperation::Test(_)) {
            continue;
        }
        let path = json_patch_operation_path(op);

        // Prevent modifying resourceType
        if path == "/resourceType" || path.starts_with("/resourceType/") {
            return Err(ApiError::bad_request(
                "Cannot modify resourceType with patch".to_string(),
            ));
        }

        // Prevent modifying id
        if path == "/id" || path.starts_with("/id/") {
            return Err(ApiError::bad_request(
                "Cannot modify id with patch".to_string(),
            ));
        }
    }
    Ok(())
}

/// Decode a Bundle PATCH payload and use the same patch engines as HTTP PATCH.
pub async fn apply_bundle_patch(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    patch: &Value,
) -> Result<Value, ApiError> {
    match patch["resourceType"].as_str() {
        Some("Binary") => {
            use base64::{Engine, engine::general_purpose::STANDARD};
            if patch["contentType"].as_str() != Some("application/json-patch+json") {
                return Err(ApiError::unsupported_media_type(
                    "Bundle Binary PATCH requires application/json-patch+json",
                ));
            }
            let data = patch["data"]
                .as_str()
                .ok_or_else(|| ApiError::bad_request("Binary PATCH requires data"))?;
            let bytes = STANDARD
                .decode(data)
                .map_err(|e| ApiError::bad_request(format!("Invalid Binary PATCH data: {e}")))?;
            apply_json_patch(resource, &bytes)
        }
        Some("Parameters") => {
            let bytes =
                serde_json::to_vec(patch).map_err(|e| ApiError::bad_request(e.to_string()))?;
            apply_fhirpath_patch(engine, model_provider, resource, &bytes).await
        }
        _ => Err(ApiError::bad_request(
            "Bundle PATCH requires Binary or FHIRPath Parameters",
        )),
    }
}

/// Extracts the path from a JSON Patch operation.
fn json_patch_operation_path(op: &PatchOperation) -> &str {
    match op {
        PatchOperation::Add(add_op) => add_op.path.as_str(),
        PatchOperation::Remove(remove_op) => remove_op.path.as_str(),
        PatchOperation::Replace(replace_op) => replace_op.path.as_str(),
        PatchOperation::Move(move_op) => move_op.path.as_str(),
        PatchOperation::Copy(copy_op) => copy_op.path.as_str(),
        PatchOperation::Test(test_op) => test_op.path.as_str(),
    }
}

// ============================================================================
// FHIRPath Patch
// ============================================================================

/// Applies a FHIRPath Patch to a FHIR resource.
///
/// FHIRPath Patch uses a Parameters resource containing operations that use
/// FHIRPath expressions to locate elements to modify.
///
/// # Arguments
/// * `engine` - The FHIRPath evaluation engine
/// * `model_provider` - The FHIR model provider for schema-aware evaluation
/// * `resource` - The current resource JSON to patch
/// * `patch_bytes` - Raw FHIRPath Patch Parameters resource bytes
///
/// # Returns
/// The patched resource or an error if the patch is invalid or fails to apply
pub async fn apply_fhirpath_patch(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    patch_bytes: &[u8],
) -> Result<Value, ApiError> {
    // Parse Parameters resource
    let parameters: Value = serde_json::from_slice(patch_bytes)
        .map_err(|e| ApiError::bad_request(format!("Invalid FHIRPath Patch document: {e}")))?;

    // Validate it's a Parameters resource
    if parameters.get("resourceType").and_then(|v| v.as_str()) != Some("Parameters") {
        return Err(ApiError::bad_request(
            "FHIRPath Patch must be a Parameters resource".to_string(),
        ));
    }

    let mut patched = resource.clone();

    // Get the parameter array
    let params = parameters
        .get("parameter")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApiError::bad_request("Missing parameter array in FHIRPath Patch"))?;

    // Process each operation
    for param in params {
        if param.get("name").and_then(|v| v.as_str()) != Some("operation") {
            continue;
        }

        let parts = param
            .get("part")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ApiError::bad_request("Missing 'part' in operation parameter"))?;

        // Extract operation type
        let op_type = find_part_string(parts, "type")
            .ok_or_else(|| ApiError::bad_request("Missing 'type' in FHIRPath Patch operation"))?;

        // Extract path
        let path_expr = find_part_string(parts, "path")
            .ok_or_else(|| ApiError::bad_request("Missing 'path' in FHIRPath Patch operation"))?;

        // Validate path doesn't target protected fields
        validate_fhirpath_path(&path_expr)?;

        // Apply the operation
        match op_type.as_str() {
            "add" => {
                let name = find_part_string(parts, "name").ok_or_else(|| {
                    ApiError::bad_request("'add' operation requires 'name' parameter")
                })?;
                let value = find_part_value(parts).ok_or_else(|| {
                    ApiError::bad_request("'add' operation requires a value parameter")
                })?;
                patched = fhirpath_add(engine, model_provider, &patched, &path_expr, &name, &value)
                    .await?;
            }
            "insert" => {
                let index = find_part_integer(parts, "index").ok_or_else(|| {
                    ApiError::bad_request("'insert' operation requires 'index' parameter")
                })?;
                let value = find_part_value(parts).ok_or_else(|| {
                    ApiError::bad_request("'insert' operation requires a value parameter")
                })?;
                patched = fhirpath_insert(
                    engine,
                    model_provider,
                    &patched,
                    &path_expr,
                    nonnegative_index(index)?,
                    &value,
                )
                .await?;
            }
            "delete" => {
                patched = fhirpath_delete(engine, model_provider, &patched, &path_expr).await?;
            }
            "replace" => {
                let value = find_part_value(parts).ok_or_else(|| {
                    ApiError::bad_request("'replace' operation requires a value parameter")
                })?;
                patched =
                    fhirpath_replace(engine, model_provider, &patched, &path_expr, &value).await?;
            }
            "move" => {
                let source = find_part_integer(parts, "source").ok_or_else(|| {
                    ApiError::bad_request("'move' operation requires 'source' parameter")
                })?;
                let destination = find_part_integer(parts, "destination").ok_or_else(|| {
                    ApiError::bad_request("'move' operation requires 'destination' parameter")
                })?;
                patched = fhirpath_move(
                    engine,
                    model_provider,
                    &patched,
                    &path_expr,
                    nonnegative_index(source)?,
                    nonnegative_index(destination)?,
                )
                .await?;
            }
            _ => {
                return Err(ApiError::bad_request(format!(
                    "Unknown FHIRPath Patch operation type: {}",
                    op_type
                )));
            }
        }
    }

    if patched.get("id") != resource.get("id")
        || patched.get("resourceType") != resource.get("resourceType")
    {
        return Err(ApiError::bad_request(
            "Patch cannot change resource identity",
        ));
    }
    Ok(patched)
}

fn nonnegative_index(value: i64) -> Result<usize, ApiError> {
    usize::try_from(value).map_err(|_| ApiError::bad_request("Patch indices must be nonnegative"))
}

/// Validates that the FHIRPath expression doesn't target protected fields.
fn validate_fhirpath_path(path: &str) -> Result<(), ApiError> {
    let path_lower = path.to_lowercase();
    if path_lower == "resourcetype"
        || path_lower.starts_with("resourcetype.")
        || path_lower.ends_with(".resourcetype")
    {
        return Err(ApiError::bad_request(
            "Cannot modify resourceType with FHIRPath Patch".to_string(),
        ));
    }
    // Note: We allow patching 'id' in nested resources, just not the root id
    // The root id check is done at a higher level
    Ok(())
}

/// Find a string value in the parts array by name.
fn find_part_string(parts: &[Value], name: &str) -> Option<String> {
    parts.iter().find_map(|p| {
        if p.get("name")?.as_str()? == name {
            // Try valueString, valueCode, valueUri
            p.get("valueString")
                .or_else(|| p.get("valueCode"))
                .or_else(|| p.get("valueUri"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Find an integer value in the parts array by name.
fn find_part_integer(parts: &[Value], name: &str) -> Option<i64> {
    parts.iter().find_map(|p| {
        if p.get("name")?.as_str()? == name {
            p.get("valueInteger").and_then(|v| v.as_i64())
        } else {
            None
        }
    })
}

/// Find a value parameter (valueXxx) in the parts array.
fn find_part_value(parts: &[Value]) -> Option<Value> {
    parts.iter().find_map(|p| {
        let name = p.get("name")?.as_str()?;
        if name.starts_with("value") {
            // This part IS the value - extract the actual value
            p.as_object()?.iter().find_map(|(k, v)| {
                if k.starts_with("value") && k != "name" {
                    Some(v.clone())
                } else {
                    None
                }
            })
        } else {
            None
        }
    })
}

/// Require a single resource location, rather than matching by equal JSON values.
async fn single_location(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    allow_missing: bool,
) -> Result<Option<locations::Location>, ApiError> {
    let mut targets = locations::select(engine, provider, resource, path).await?;
    if targets.len() > 1 || (targets.is_empty() && !allow_missing) {
        return Err(ApiError::bad_request(format!(
            "Patch path '{path}' must select exactly one element"
        )));
    }
    Ok(targets.pop())
}

async fn fhirpath_add(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    name: &str,
    value: &Value,
) -> Result<Value, ApiError> {
    let target = single_location(engine, provider, resource, path, false)
        .await?
        .unwrap();
    let element = provider
        .get_element_type(target.value.type_info(), name)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .ok_or_else(|| ApiError::bad_request(format!("Unknown patch element '{name}'")))?;
    let mut result = resource.clone();
    let object = result
        .pointer_mut(&target.pointer)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ApiError::bad_request("Patch add requires an object target"))?;
    if element.singleton == Some(false) {
        let array = object
            .entry(name)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| ApiError::bad_request("Repeating patch element must be an array"))?;
        array.push(value.clone());
    } else {
        if object.contains_key(name) {
            return Err(ApiError::bad_request(
                "Patch add cannot replace an existing non-repeating element",
            ));
        }
        object.insert(name.to_owned(), value.clone());
    }
    Ok(result)
}

/// A collection operation must select one physical JSON array, including an empty one.
async fn collection_pointer(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
) -> Result<String, ApiError> {
    use octofhir_fhirpath::ast::ExpressionNode;
    let ast = engine
        .compile_ast(path)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (parent_path, name) = match ast.as_ref() {
        ExpressionNode::PropertyAccess(node) => (node.object.to_string(), node.property.clone()),
        ExpressionNode::Identifier(node) => (
            resource["resourceType"].as_str().unwrap_or("").to_owned(),
            node.name.clone(),
        ),
        _ => {
            return Err(ApiError::bad_request(
                "Collection patch path must select a repeating property",
            ));
        }
    };
    let parent = single_location(engine, provider, resource, &parent_path, false)
        .await?
        .unwrap();
    let pointer = format!("{}/{}", parent.pointer, locations::escape(&name));
    if !resource.pointer(&pointer).is_some_and(Value::is_array) {
        return Err(ApiError::bad_request(
            "Collection patch target must be an array",
        ));
    }
    Ok(pointer)
}

async fn fhirpath_insert(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    index: usize,
    value: &Value,
) -> Result<Value, ApiError> {
    let pointer = collection_pointer(engine, provider, resource, path).await?;
    let mut result = resource.clone();
    let array = result
        .pointer_mut(&pointer)
        .and_then(Value::as_array_mut)
        .unwrap();
    if index > array.len() {
        return Err(ApiError::bad_request("Patch insert index is out of bounds"));
    }
    array.insert(index, value.clone());
    Ok(result)
}

async fn fhirpath_delete(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
) -> Result<Value, ApiError> {
    let Some(target) = single_location(engine, provider, resource, path, true).await? else {
        return Ok(resource.clone());
    };
    let (parent, key) = target
        .pointer
        .rsplit_once('/')
        .ok_or_else(|| ApiError::bad_request("Patch cannot delete the resource root"))?;
    let mut result = resource.clone();
    match result.pointer_mut(parent) {
        Some(Value::Array(array)) => {
            let index = key
                .parse::<usize>()
                .map_err(|_| ApiError::bad_request("Invalid patch array index"))?;
            array.remove(index);
        }
        Some(Value::Object(object)) => {
            object.remove(&key.replace("~1", "/").replace("~0", "~"));
        }
        _ => return Err(ApiError::bad_request("Patch delete target does not exist")),
    }
    // FHIR JSON omits empty arrays, including arrays emptied by a delete.
    if result
        .pointer(parent)
        .is_some_and(|v| v.as_array().is_some_and(Vec::is_empty))
        && let Some((grandparent, field)) = parent.rsplit_once('/')
        && let Some(object) = result
            .pointer_mut(grandparent)
            .and_then(Value::as_object_mut)
    {
        object.remove(&field.replace("~1", "/").replace("~0", "~"));
    }
    Ok(result)
}

async fn fhirpath_replace(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    value: &Value,
) -> Result<Value, ApiError> {
    let target = single_location(engine, provider, resource, path, false)
        .await?
        .unwrap();
    let mut result = resource.clone();
    *result
        .pointer_mut(&target.pointer)
        .ok_or_else(|| ApiError::bad_request("Patch target does not exist"))? = value.clone();
    Ok(result)
}

async fn fhirpath_move(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    source: usize,
    destination: usize,
) -> Result<Value, ApiError> {
    let pointer = collection_pointer(engine, provider, resource, path).await?;
    let mut result = resource.clone();
    let array = result
        .pointer_mut(&pointer)
        .and_then(Value::as_array_mut)
        .unwrap();
    if source >= array.len() || destination >= array.len() {
        return Err(ApiError::bad_request("Patch move index is out of bounds"));
    }
    let value = array.remove(source);
    array.insert(destination, value);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_apply_json_patch_add() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123",
            "name": [{"family": "Doe"}]
        });

        let patch = r#"[{"op": "add", "path": "/birthDate", "value": "1990-01-01"}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes()).unwrap();

        assert_eq!(result["birthDate"], "1990-01-01");
        assert_eq!(result["resourceType"], "Patient");
        assert_eq!(result["id"], "123");
    }

    #[test]
    fn test_apply_json_patch_replace() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123",
            "active": false
        });

        let patch = r#"[{"op": "replace", "path": "/active", "value": true}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes()).unwrap();

        assert_eq!(result["active"], true);
    }

    #[test]
    fn test_apply_json_patch_remove() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123",
            "active": true,
            "birthDate": "1990-01-01"
        });

        let patch = r#"[{"op": "remove", "path": "/birthDate"}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes()).unwrap();

        assert!(result.get("birthDate").is_none());
        assert_eq!(result["active"], true);
    }

    #[test]
    fn test_reject_patch_resource_type() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let patch = r#"[{"op": "replace", "path": "/resourceType", "value": "Observation"}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn test_reject_patch_id() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let patch = r#"[{"op": "replace", "path": "/id", "value": "456"}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn test_invalid_json_patch() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let patch = r#"not valid json"#;
        let result = apply_json_patch(&resource, patch.as_bytes());

        assert!(result.is_err());
    }

    #[test]
    fn test_patch_nonexistent_path() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        // Replace on non-existent path should fail
        let patch = r#"[{"op": "replace", "path": "/nonexistent", "value": "test"}]"#;
        let result = apply_json_patch(&resource, patch.as_bytes());

        assert!(result.is_err());
    }
}
