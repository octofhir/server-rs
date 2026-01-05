//! CustomOperation reconciliation logic.

use std::collections::{HashMap, HashSet};

use octofhir_api::ApiError;
use octofhir_storage::{DynStorage, SearchParams, StoredResource};

use crate::app_platform::AppEndpoint;
use crate::gateway::types::{CustomOperation, InlineOperation, ProxyConfig, Reference};

/// Result of reconciliation operation containing counts of changes.
#[derive(Debug, Default)]
pub struct ReconcileResult {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
}

/// Reconcile CustomOperations based on App.operations[].
///
/// This function performs a three-way diff between existing CustomOperations
/// in the database and the operations defined in the App resource:
/// - Creates new operations that don't exist
/// - Updates existing operations if their definitions changed
/// - Deletes operations that are no longer in the App
///
/// # Arguments
///
/// * `storage` - Storage backend for FHIR resources
/// * `app_id` - ID of the App resource
/// * `app_name` - Name of the App (for display in references)
/// * `app_endpoint` - Backend endpoint configuration
/// * `inline_operations` - Operations from App.operations[]
///
/// # Returns
///
/// `ReconcileResult` containing lists of created, updated, and deleted operation IDs.
pub async fn reconcile_operations(
    storage: &DynStorage,
    app_id: &str,
    app_name: &str,
    app_endpoint: &Option<AppEndpoint>,
    inline_operations: &[InlineOperation],
) -> Result<ReconcileResult, ApiError> {
    let mut result = ReconcileResult::default();

    // 1. Load existing CustomOperations for this app
    let current_ops = load_app_operations(storage, app_id).await?;
    let current_map: HashMap<String, CustomOperation> = current_ops
        .into_iter()
        .map(|op| {
            let op_id = extract_op_id(&op.id);
            (op_id, op)
        })
        .collect();

    // 2. Build manifest operations map
    let manifest_map: HashMap<String, &InlineOperation> = inline_operations
        .iter()
        .map(|op| (op.id.clone(), op))
        .collect();

    let manifest_ids: HashSet<&String> = manifest_map.keys().collect();
    let current_ids: HashSet<String> = current_map.keys().cloned().collect();
    let current_ids_ref: HashSet<&String> = current_ids.iter().collect();

    // 3. CREATE new operations
    for op_id in manifest_ids.difference(&current_ids_ref) {
        let inline_op = manifest_map[*op_id];
        let custom_op = build_custom_operation(app_id, app_name, app_endpoint, inline_op)?;
        let resource_json = serde_json::to_value(&custom_op)
            .map_err(|e| ApiError::internal(format!("Failed to serialize CustomOperation: {}", e)))?;
        storage.create(&resource_json).await
            .map_err(|e| ApiError::internal(format!("Failed to create CustomOperation: {}", e)))?;
        result.created.push((*op_id).clone());
    }

    // 4. UPDATE changed operations
    for op_id in manifest_ids.intersection(&current_ids_ref) {
        let inline_op = manifest_map[*op_id];
        let current_op = &current_map[*op_id];

        if needs_update(current_op, inline_op, app_endpoint) {
            let custom_op = build_custom_operation(app_id, app_name, app_endpoint, inline_op)?;
            let resource_json = serde_json::to_value(&custom_op)
                .map_err(|e| ApiError::internal(format!("Failed to serialize CustomOperation: {}", e)))?;
            storage.update(&resource_json, None).await
                .map_err(|e| ApiError::internal(format!("Failed to update CustomOperation: {}", e)))?;
            result.updated.push((*op_id).clone());
        }
    }

    // 5. DELETE removed operations
    for op_id in current_ids_ref.difference(&manifest_ids) {
        let full_id = format!("{}-{}", app_id, op_id);
        storage.delete("CustomOperation", &full_id).await
            .map_err(|e| ApiError::internal(format!("Failed to delete CustomOperation: {}", e)))?;
        result.deleted.push((*op_id).clone());
    }

    tracing::info!(
        app_id = %app_id,
        created = result.created.len(),
        updated = result.updated.len(),
        deleted = result.deleted.len(),
        "CustomOperations reconciled"
    );

    Ok(result)
}

/// Load all CustomOperations for a given App from storage.
async fn load_app_operations(
    storage: &DynStorage,
    app_id: &str,
) -> Result<Vec<CustomOperation>, ApiError> {
    let search_params = SearchParams::new().with_count(1000);
    let search_result = storage
        .search("CustomOperation", &search_params)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to search CustomOperations: {}", e)))?;

    let app_ref = format!("App/{}", app_id);
    let operations: Vec<CustomOperation> = search_result
        .entries
        .into_iter()
        .filter_map(|stored: StoredResource| {
            serde_json::from_value::<CustomOperation>(stored.resource).ok()
        })
        .filter(|op| {
            op.app
                .reference
                .as_ref()
                .map(|r| r == &app_ref)
                .unwrap_or(false)
        })
        .collect();

    Ok(operations)
}

/// Build a CustomOperation from an InlineOperation.
///
/// This mirrors the logic in `GatewayRouter::inline_to_custom_operation`
/// from router.rs.
fn build_custom_operation(
    app_id: &str,
    app_name: &str,
    app_endpoint: &Option<AppEndpoint>,
    inline_op: &InlineOperation,
) -> Result<CustomOperation, ApiError> {
    // Build proxy config based on operation type
    let proxy = if inline_op.operation_type == "websocket" {
        // WebSocket operations use websocket config
        Some(ProxyConfig {
            url: String::new(), // Not used for websocket
            timeout: None,
            forward_auth: None,
            headers: None,
            websocket: inline_op.websocket.clone(),
        })
    } else {
        // Regular app operations use endpoint config
        app_endpoint.as_ref().map(|ep| ProxyConfig {
            url: ep.url.clone(),
            timeout: ep.timeout,
            forward_auth: Some(true),
            headers: None,
            websocket: None,
        })
    };

    Ok(CustomOperation {
        id: Some(format!("{}-{}", app_id, inline_op.id)),
        resource_type: "CustomOperation".to_string(),
        app: Reference {
            reference: Some(format!("App/{}", app_id)),
            display: Some(app_name.to_string()),
        },
        path: inline_op.path_string(),
        method: inline_op.method.to_string(),
        operation_type: inline_op.operation_type.clone(),
        active: true,
        public: inline_op.public,
        policy: inline_op.policy.clone(),
        proxy,
        sql: None,
        fhirpath: None,
        handler: None,
        include_raw_body: inline_op.include_raw_body,
    })
}

/// Check if a CustomOperation needs to be updated based on InlineOperation.
fn needs_update(
    current: &CustomOperation,
    inline_op: &InlineOperation,
    app_endpoint: &Option<AppEndpoint>,
) -> bool {
    let new_path = inline_op.path_string();
    let new_method = inline_op.method.to_string();

    // Build expected proxy config based on operation type
    let new_proxy = if inline_op.operation_type == "websocket" {
        Some(ProxyConfig {
            url: String::new(),
            timeout: None,
            forward_auth: None,
            headers: None,
            websocket: inline_op.websocket.clone(),
        })
    } else {
        app_endpoint.as_ref().map(|ep| ProxyConfig {
            url: ep.url.clone(),
            timeout: ep.timeout,
            forward_auth: Some(true),
            headers: None,
            websocket: None,
        })
    };

    current.path != new_path
        || current.method != new_method
        || current.operation_type != inline_op.operation_type
        || current.public != inline_op.public
        || current.policy != inline_op.policy
        || current.proxy != new_proxy
        || current.include_raw_body != inline_op.include_raw_body
}

/// Extract operation ID from full CustomOperation ID.
///
/// Full ID format: "{app_id}-{operation_id}"
/// Example: "psychportal-book-appointment" -> "book-appointment"
fn extract_op_id(full_id: &Option<String>) -> String {
    full_id
        .as_ref()
        .and_then(|id| {
            // Find the first '-' and take everything after it
            id.find('-').map(|idx| &id[idx + 1..])
        })
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_platform::HttpMethod;

    #[test]
    fn test_extract_op_id() {
        assert_eq!(
            extract_op_id(&Some("psychportal-book-appointment".to_string())),
            "book-appointment"
        );
        assert_eq!(
            extract_op_id(&Some("app-id-with-dashes-op-id".to_string())),
            "id-with-dashes-op-id"
        );
        assert_eq!(extract_op_id(&Some("no-dash".to_string())), "dash");
        assert_eq!(extract_op_id(&None), "");
    }

    #[test]
    fn test_build_custom_operation() {
        let inline_op = InlineOperation {
            id: "test-op".to_string(),
            method: HttpMethod::Post,
            path: vec!["api".to_string(), "users".to_string()],
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        };

        let endpoint = Some(AppEndpoint {
            url: "http://backend:3000".to_string(),
            timeout: Some(30),
        });

        let custom_op =
            build_custom_operation("test-app", "Test App", &endpoint, &inline_op).unwrap();

        assert_eq!(custom_op.id, Some("test-app-test-op".to_string()));
        assert_eq!(custom_op.resource_type, "CustomOperation");
        assert_eq!(
            custom_op.app.reference,
            Some("App/test-app".to_string())
        );
        assert_eq!(custom_op.app.display, Some("Test App".to_string()));
        assert_eq!(custom_op.path, "/api/users");
        assert_eq!(custom_op.method, "POST");
        assert_eq!(custom_op.operation_type, "app");
        assert_eq!(custom_op.active, true);
        assert_eq!(custom_op.public, false);
        assert!(custom_op.proxy.is_some());
        assert_eq!(custom_op.proxy.as_ref().unwrap().url, "http://backend:3000");
        assert_eq!(custom_op.proxy.as_ref().unwrap().timeout, Some(30));
        assert_eq!(custom_op.proxy.as_ref().unwrap().forward_auth, Some(true));
    }

    #[test]
    fn test_needs_update_no_changes() {
        let inline_op = InlineOperation {
            id: "test-op".to_string(),
            method: HttpMethod::Get,
            path: vec!["users".to_string()],
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        };

        let endpoint = Some(AppEndpoint {
            url: "http://backend:3000".to_string(),
            timeout: Some(30),
        });

        let custom_op =
            build_custom_operation("test-app", "Test App", &endpoint, &inline_op).unwrap();

        assert!(!needs_update(&custom_op, &inline_op, &endpoint));
    }

    #[test]
    fn test_needs_update_path_changed() {
        let inline_op = InlineOperation {
            id: "test-op".to_string(),
            method: HttpMethod::Get,
            path: vec!["users".to_string()],
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        };

        let endpoint = Some(AppEndpoint {
            url: "http://backend:3000".to_string(),
            timeout: Some(30),
        });

        let mut custom_op =
            build_custom_operation("test-app", "Test App", &endpoint, &inline_op).unwrap();

        // Change path
        custom_op.path = "/api/users".to_string();

        assert!(needs_update(&custom_op, &inline_op, &endpoint));
    }

    #[test]
    fn test_needs_update_method_changed() {
        let inline_op = InlineOperation {
            id: "test-op".to_string(),
            method: HttpMethod::Post,
            path: vec!["users".to_string()],
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        };

        let endpoint = Some(AppEndpoint {
            url: "http://backend:3000".to_string(),
            timeout: Some(30),
        });

        let mut custom_op =
            build_custom_operation("test-app", "Test App", &endpoint, &inline_op).unwrap();

        // Change method
        custom_op.method = "GET".to_string();

        assert!(needs_update(&custom_op, &inline_op, &endpoint));
    }

    #[test]
    fn test_needs_update_public_changed() {
        let inline_op = InlineOperation {
            id: "test-op".to_string(),
            method: HttpMethod::Get,
            path: vec!["users".to_string()],
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        };

        let endpoint = Some(AppEndpoint {
            url: "http://backend:3000".to_string(),
            timeout: Some(30),
        });

        let mut custom_op =
            build_custom_operation("test-app", "Test App", &endpoint, &inline_op).unwrap();

        // Change public flag
        custom_op.public = true;

        assert!(needs_update(&custom_op, &inline_op, &endpoint));
    }
}
