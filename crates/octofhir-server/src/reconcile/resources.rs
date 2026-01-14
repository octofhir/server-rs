//! Resource reconciliation logic for App.resources.
//!
//! Handles provisioning of FHIR resources defined in App Manifest,
//! such as OAuth Clients, Access Policies, and other resources.

use std::collections::HashMap;

use octofhir_api::ApiError;
use octofhir_auth::hash_password;
use octofhir_storage::DynStorage;
use serde_json::Value;

/// Reconcile resources defined in App Manifest.
///
/// Resources are provisioned (created or updated) based on the App.resources field.
/// Each resource is tagged with the app_id in meta.tag for tracking and cleanup.
///
/// Supported resource types:
/// - Client (OAuth client)
/// - AccessPolicy (authorization policy)
/// - Any other FHIR resource types
///
/// # Arguments
///
/// * `storage` - Storage backend for FHIR resources
/// * `app_id` - ID of the App resource
/// * `resources` - Resources from App.resources (deserialized from JSON string)
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if reconciliation fails.
///
/// # Example
///
/// ```ignore
/// let resources = hashmap! {
///     "Client".to_string() => hashmap! {
///         "my-client".to_string() => json!({
///             "name": "My Client",
///             "grant_types": ["authorization_code"]
///         })
///     }
/// };
///
/// reconcile_resources(&storage, "my-app", &resources).await?;
/// ```
pub async fn reconcile_resources(
    storage: &DynStorage,
    app_id: &str,
    resources: &HashMap<String, HashMap<String, Value>>,
) -> Result<(), ApiError> {
    for (resource_type, items) in resources {
        match resource_type.as_str() {
            "Client" => reconcile_resource_type(storage, app_id, "Client", items).await?,
            "AccessPolicy" => {
                reconcile_resource_type(storage, app_id, "AccessPolicy", items).await?
            }
            // Support any other resource types
            _ => {
                tracing::warn!(
                    app_id = %app_id,
                    resource_type = %resource_type,
                    "Unknown resource type in App manifest, attempting to reconcile anyway"
                );
                reconcile_resource_type(storage, app_id, resource_type, items).await?;
            }
        }
    }

    Ok(())
}

/// Preprocess resource payload before storage (hash passwords/secrets).
///
/// This mirrors the logic in handlers.rs preprocess_payload to ensure
/// resources created via App.resources have properly hashed credentials.
fn preprocess_resource(resource_type: &str, resource: &mut Value) -> Result<(), ApiError> {
    let Some(obj) = resource.as_object_mut() else {
        return Ok(());
    };

    match resource_type {
        "User" => {
            // Hash password -> passwordHash
            if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
                let hashed = hash_password(password)
                    .map_err(|e| ApiError::internal(format!("Failed to hash password: {}", e)))?;
                obj.insert("passwordHash".to_string(), Value::String(hashed));
                obj.remove("password");
            }
        }
        "Client" => {
            // Hash clientSecret
            if let Some(secret) = obj.get("clientSecret").and_then(|v| v.as_str()) {
                // Only hash if not already hashed
                if !secret.starts_with("$argon2id$") {
                    let hashed = hash_password(secret).map_err(|e| {
                        ApiError::internal(format!("Failed to hash client secret: {}", e))
                    })?;
                    obj.insert("clientSecret".to_string(), Value::String(hashed));
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// Reconcile resources of a specific type.
async fn reconcile_resource_type(
    storage: &DynStorage,
    app_id: &str,
    resource_type: &str,
    items: &HashMap<String, Value>,
) -> Result<(), ApiError> {
    for (resource_id, resource_def) in items {
        // Parse resource definition
        let mut resource: Value = resource_def.clone();

        // Ensure resourceType is set
        if let Some(obj) = resource.as_object_mut() {
            obj.insert(
                "resourceType".to_string(),
                Value::String(resource_type.to_string()),
            );
            obj.insert("id".to_string(), Value::String(resource_id.clone()));

            // Add meta.tag to track app ownership
            let meta = obj.entry("meta").or_insert_with(|| serde_json::json!({}));
            if let Some(meta_obj) = meta.as_object_mut() {
                let tags = meta_obj
                    .entry("tag")
                    .or_insert_with(|| serde_json::json!([]));
                if let Some(tags_array) = tags.as_array_mut() {
                    // Add app tag if not already present
                    let app_tag = serde_json::json!({
                        "system": "octofhir/app",
                        "code": app_id
                    });
                    if !tags_array.contains(&app_tag) {
                        tags_array.push(app_tag);
                    }
                }
            }
        }

        // Hash passwords/secrets before storage
        preprocess_resource(resource_type, &mut resource)?;

        // Check if resource exists
        let existing = storage
            .read(resource_type, resource_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to read {}: {}", resource_type, e)))?;

        if existing.is_some() {
            // Update existing resource
            storage.update(&resource, None).await.map_err(|e| {
                ApiError::internal(format!("Failed to update {}: {}", resource_type, e))
            })?;

            tracing::info!(
                app_id = %app_id,
                resource_type = %resource_type,
                resource_id = %resource_id,
                "Updated provisioned resource"
            );
        } else {
            // Create new resource
            storage.create(&resource).await.map_err(|e| {
                ApiError::internal(format!("Failed to create {}: {}", resource_type, e))
            })?;

            tracing::info!(
                app_id = %app_id,
                resource_type = %resource_type,
                resource_id = %resource_id,
                "Created provisioned resource"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_meta_tag() {
        let mut resource = serde_json::json!({
            "resourceType": "Client",
            "id": "test-client",
            "name": "Test Client"
        });

        // Simulate adding meta tag
        if let Some(obj) = resource.as_object_mut() {
            let meta = obj.entry("meta").or_insert_with(|| serde_json::json!({}));
            if let Some(meta_obj) = meta.as_object_mut() {
                let tags = meta_obj
                    .entry("tag")
                    .or_insert_with(|| serde_json::json!([]));
                if let Some(tags_array) = tags.as_array_mut() {
                    tags_array.push(serde_json::json!({
                        "system": "octofhir/app",
                        "code": "test-app"
                    }));
                }
            }
        }

        assert!(resource["meta"]["tag"].is_array());
        assert_eq!(resource["meta"]["tag"].as_array().unwrap().len(), 1);
        assert_eq!(resource["meta"]["tag"][0]["system"], "octofhir/app");
        assert_eq!(resource["meta"]["tag"][0]["code"], "test-app");
    }

    #[test]
    fn test_resource_type_and_id_injection() {
        let resource_def = serde_json::json!({
            "name": "Test Client",
            "grant_types": ["authorization_code"]
        });

        let mut resource = resource_def.clone();

        // Simulate injection
        if let Some(obj) = resource.as_object_mut() {
            obj.insert(
                "resourceType".to_string(),
                Value::String("Client".to_string()),
            );
            obj.insert("id".to_string(), Value::String("test-client".to_string()));
        }

        assert_eq!(resource["resourceType"], "Client");
        assert_eq!(resource["id"], "test-client");
        assert_eq!(resource["name"], "Test Client");
    }
}
