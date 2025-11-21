//! JSON Patch (RFC 6902) and FHIRPath Patch implementation for FHIR resources.

use std::sync::Arc;

use json_patch::{Patch, PatchOperation, patch};
use octofhir_api::ApiError;
use octofhir_fhir_model::provider::ModelProvider;
use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, FhirPathValue};
use serde_json::Value;

/// Type alias for shared model provider
pub type SharedModelProvider = Arc<dyn ModelProvider + Send + Sync>;

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
                    index as usize,
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
                patched =
                    fhirpath_move(&patched, &path_expr, source as usize, destination as usize)
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

    Ok(patched)
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

/// Execute FHIRPath 'add' operation - adds a new element to an object.
async fn fhirpath_add(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    name: &str,
    value: &Value,
) -> Result<Value, ApiError> {
    let mut result = resource.clone();

    // Evaluate the FHIRPath to find the target location
    let targets = evaluate_fhirpath(engine, model_provider, resource, path).await?;

    if targets.is_empty() {
        return Err(ApiError::bad_request(format!(
            "FHIRPath expression '{}' did not match any elements",
            path
        )));
    }

    // For simplicity, we handle the common case of adding to root or a direct child
    // A full implementation would need to track JSON paths during evaluation
    if path == "$this" || path.chars().all(|c| c.is_alphanumeric()) {
        // Adding to root or simple field
        if let Some(obj) = result.as_object_mut() {
            obj.insert(name.to_string(), value.clone());
        }
    } else {
        // For complex paths, use a simplified approach
        // This handles paths like "Patient.name" by navigating the JSON
        let segments: Vec<&str> = path.split('.').collect();
        if let Some(target) = navigate_to_parent(&mut result, &segments)
            && let Some(obj) = target.as_object_mut()
        {
            obj.insert(name.to_string(), value.clone());
        }
    }

    Ok(result)
}

/// Execute FHIRPath 'insert' operation - inserts into an array at a specific index.
async fn fhirpath_insert(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    index: usize,
    value: &Value,
) -> Result<Value, ApiError> {
    let mut result = resource.clone();

    // Evaluate path to verify it exists
    let _targets = evaluate_fhirpath(engine, model_provider, resource, path).await?;

    // Navigate to the array and insert
    let segments: Vec<&str> = path.split('.').collect();
    if let Some(target) = navigate_to_path(&mut result, &segments) {
        if let Some(arr) = target.as_array_mut() {
            if index <= arr.len() {
                arr.insert(index, value.clone());
            } else {
                return Err(ApiError::bad_request(format!(
                    "Index {} out of bounds for array of length {}",
                    index,
                    arr.len()
                )));
            }
        } else {
            return Err(ApiError::bad_request(format!(
                "Path '{}' does not point to an array",
                path
            )));
        }
    }

    Ok(result)
}

/// Execute FHIRPath 'delete' operation - removes an element.
async fn fhirpath_delete(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
) -> Result<Value, ApiError> {
    let mut result = resource.clone();

    // Evaluate path to verify it exists
    let _targets = evaluate_fhirpath(engine, model_provider, resource, path).await?;

    // Navigate to parent and remove the field
    let segments: Vec<&str> = path.split('.').collect();
    if segments.len() >= 2 {
        let parent_segments = &segments[..segments.len() - 1];
        let field_name = segments[segments.len() - 1];

        if let Some(parent) = navigate_to_path(&mut result, parent_segments)
            && let Some(obj) = parent.as_object_mut()
        {
            obj.remove(field_name);
        }
    } else if segments.len() == 1 {
        // Deleting from root
        if let Some(obj) = result.as_object_mut() {
            obj.remove(segments[0]);
        }
    }

    Ok(result)
}

/// Execute FHIRPath 'replace' operation - replaces an element's value.
async fn fhirpath_replace(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
    value: &Value,
) -> Result<Value, ApiError> {
    let mut result = resource.clone();

    // Evaluate path to verify it exists
    let targets = evaluate_fhirpath(engine, model_provider, resource, path).await?;

    if targets.is_empty() {
        return Err(ApiError::bad_request(format!(
            "FHIRPath expression '{}' did not match any elements for replace",
            path
        )));
    }

    // Navigate to parent and replace the field
    let segments: Vec<&str> = path.split('.').collect();
    if segments.len() >= 2 {
        let parent_segments = &segments[..segments.len() - 1];
        let field_name = segments[segments.len() - 1];

        if let Some(parent) = navigate_to_path(&mut result, parent_segments)
            && let Some(obj) = parent.as_object_mut()
        {
            obj.insert(field_name.to_string(), value.clone());
        }
    } else if segments.len() == 1 {
        // Replacing at root level
        if let Some(obj) = result.as_object_mut() {
            obj.insert(segments[0].to_string(), value.clone());
        }
    }

    Ok(result)
}

/// Execute FHIRPath 'move' operation - moves an element within an array.
async fn fhirpath_move(
    resource: &Value,
    path: &str,
    source: usize,
    destination: usize,
) -> Result<Value, ApiError> {
    let mut result = resource.clone();

    let segments: Vec<&str> = path.split('.').collect();
    if let Some(target) = navigate_to_path(&mut result, &segments) {
        if let Some(arr) = target.as_array_mut() {
            if source >= arr.len() {
                return Err(ApiError::bad_request(format!(
                    "Source index {} out of bounds for array of length {}",
                    source,
                    arr.len()
                )));
            }
            let elem = arr.remove(source);
            let dest = if destination > source {
                destination - 1
            } else {
                destination
            };
            if dest > arr.len() {
                arr.push(elem);
            } else {
                arr.insert(dest, elem);
            }
        } else {
            return Err(ApiError::bad_request(format!(
                "Path '{}' does not point to an array",
                path
            )));
        }
    }

    Ok(result)
}

/// Evaluate a FHIRPath expression against a resource.
async fn evaluate_fhirpath(
    engine: &FhirPathEngine,
    model_provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
) -> Result<Vec<FhirPathValue>, ApiError> {
    // Cast to the FHIRPath ModelProvider trait
    let fhirpath_provider: Arc<dyn octofhir_fhirpath::ModelProvider + Send + Sync> =
        model_provider.clone();
    let input = Collection::from_json_resource(resource.clone(), Some(fhirpath_provider.clone()))
        .await
        .map_err(|e| ApiError::bad_request(format!("Failed to create FHIRPath input: {}", e)))?;
    let context = EvaluationContext::new(input, fhirpath_provider, None, None, None);

    engine
        .evaluate(path, &context)
        .await
        .map(|r| r.value.into_vec())
        .map_err(|e| ApiError::bad_request(format!("FHIRPath evaluation failed: {}", e)))
}

/// Navigate to a path in the JSON structure, returning a mutable reference.
fn navigate_to_path<'a>(value: &'a mut Value, segments: &[&str]) -> Option<&'a mut Value> {
    let mut current = value;
    for segment in segments {
        // Skip the resource type prefix (e.g., "Patient" in "Patient.name")
        if current
            .get("resourceType")
            .and_then(|v| v.as_str())
            .map(|rt| rt == *segment)
            .unwrap_or(false)
        {
            continue;
        }

        current = current.get_mut(*segment)?;
    }
    Some(current)
}

/// Navigate to the parent of the target path.
fn navigate_to_parent<'a>(value: &'a mut Value, segments: &[&str]) -> Option<&'a mut Value> {
    if segments.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in segments {
        // Skip the resource type prefix
        if current
            .get("resourceType")
            .and_then(|v| v.as_str())
            .map(|rt| rt == *segment)
            .unwrap_or(false)
        {
            continue;
        }

        current = current.get_mut(*segment)?;
    }
    Some(current)
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
