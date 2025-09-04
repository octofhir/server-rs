use serde_json::Value;

use crate::canonical;

/// Placeholder for resource validation that can access the canonical registry.
/// This will evolve to real profile/StructureDefinition validation.
pub fn validate_resource(resource_type: &str, body: &Value) -> Result<(), String> {
    // Demonstrate registry access for acceptance: read packages for potential rules
    let _pkg_count = canonical::with_registry(|r| r.list().len()).unwrap_or(0);

    // Minimal shape checks for MVP
    let obj = body
        .as_object()
        .ok_or_else(|| "body must be a JSON object".to_string())?;
    let rt = obj
        .get("resourceType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing resourceType".to_string())?;
    if rt != resource_type {
        return Err(format!(
            "resourceType '{rt}' does not match path '{resource_type}'"
        ));
    }
    Ok(())
}
