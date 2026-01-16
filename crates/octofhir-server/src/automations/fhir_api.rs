//! FHIR API Extension for automation execution.
//!
//! Provides `fhir.create`, `fhir.read`, `fhir.update`, `fhir.delete`, `fhir.search`, `fhir.patch`
//! for FHIR operations from automations using the otter Extension system.

use octofhir_storage::{FhirStorage, SearchParams};
use otter_runtime::{Extension, OpContext, op_sync};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;
use tracing::{debug, warn};

/// Wrapper to hold FhirStorage + tokio Handle for blocking operations
pub struct FhirContext {
    pub storage: Arc<dyn FhirStorage>,
    pub handle: Handle,
}

/// Create the FHIR extension with all operations
pub fn fhir_extension() -> Extension {
    Extension::new("fhir")
        .with_ops(vec![
            op_sync("fhir_create", fhir_create),
            op_sync("fhir_read", fhir_read),
            op_sync("fhir_update", fhir_update),
            op_sync("fhir_delete", fhir_delete),
            op_sync("fhir_search", fhir_search),
            op_sync("fhir_patch", fhir_patch),
        ])
        .with_js(FHIR_JS_WRAPPER)
}

/// JavaScript wrapper that exposes ops as fhir.* methods
const FHIR_JS_WRAPPER: &str = r#"
globalThis.fhir = {
    create: function(resource) {
        return fhir_create(resource);
    },
    read: function(resourceType, id) {
        return fhir_read(resourceType, id);
    },
    update: function(resource) {
        return fhir_update(resource);
    },
    delete: function(resourceType, id) {
        return fhir_delete(resourceType, id);
    },
    search: function(resourceType, params) {
        return fhir_search(resourceType, params || {});
    },
    patch: function(resourceType, id, patch) {
        return fhir_patch(resourceType, id, patch);
    }
};
"#;

fn get_fhir_context(ctx: &OpContext) -> Result<Arc<FhirContext>, otter_runtime::JscError> {
    ctx.state()
        .get::<FhirContext>()
        .ok_or_else(|| otter_runtime::JscError::internal("FHIR context not available"))
}

fn fhir_create(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource = args
        .first()
        .cloned()
        .ok_or_else(|| otter_runtime::JscError::internal("fhir.create requires a resource object"))?;

    debug!(
        resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        "fhir.create called"
    );

    let storage = fhir_ctx.storage.clone();
    fhir_ctx
        .handle
        .block_on(async move {
            storage
                .create(&resource)
                .await
                .map(|stored| stored.resource)
                .map_err(|e| {
                    warn!(error = %e, "fhir.create failed");
                    otter_runtime::JscError::internal(format!("Failed to create resource: {}", e))
                })
        })
}

fn fhir_read(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource_type = args
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.read requires resourceType as first argument")
        })?
        .to_string();

    let id = args
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.read requires id as second argument")
        })?
        .to_string();

    debug!(resource_type = %resource_type, id = %id, "fhir.read called");

    let storage = fhir_ctx.storage.clone();
    fhir_ctx.handle.block_on(async move {
        match storage.read(&resource_type, &id).await {
            Ok(Some(stored)) => Ok(stored.resource),
            Ok(None) => Err(otter_runtime::JscError::internal(format!(
                "{}/{} not found",
                resource_type, id
            ))),
            Err(e) => {
                warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.read failed");
                Err(otter_runtime::JscError::internal(format!(
                    "Failed to read resource: {}",
                    e
                )))
            }
        }
    })
}

fn fhir_update(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource = args
        .first()
        .cloned()
        .ok_or_else(|| otter_runtime::JscError::internal("fhir.update requires a resource object"))?;

    debug!(
        resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "fhir.update called"
    );

    let storage = fhir_ctx.storage.clone();
    fhir_ctx.handle.block_on(async move {
        storage
            .update(&resource, None)
            .await
            .map(|stored| stored.resource)
            .map_err(|e| {
                warn!(error = %e, "fhir.update failed");
                otter_runtime::JscError::internal(format!("Failed to update resource: {}", e))
            })
    })
}

fn fhir_delete(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource_type = args
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.delete requires resourceType as first argument")
        })?
        .to_string();

    let id = args
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.delete requires id as second argument")
        })?
        .to_string();

    debug!(resource_type = %resource_type, id = %id, "fhir.delete called");

    let storage = fhir_ctx.storage.clone();
    fhir_ctx.handle.block_on(async move {
        storage
            .delete(&resource_type, &id)
            .await
            .map(|_| Value::Null)
            .map_err(|e| {
                warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.delete failed");
                otter_runtime::JscError::internal(format!("Failed to delete resource: {}", e))
            })
    })
}

fn fhir_search(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource_type = args
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.search requires resourceType as first argument")
        })?
        .to_string();

    let params: HashMap<String, String> = args
        .get(1)
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    debug!(resource_type = %resource_type, params = ?params, "fhir.search called");

    let storage = fhir_ctx.storage.clone();
    fhir_ctx.handle.block_on(async move {
        let mut search_params = SearchParams::new();
        for (key, value) in params {
            search_params.parameters.insert(key, vec![value]);
        }

        let result = storage.search(&resource_type, &search_params).await.map_err(|e| {
            warn!(error = %e, resource_type = %resource_type, "fhir.search failed");
            otter_runtime::JscError::internal(format!("Failed to search resources: {}", e))
        })?;

        let entries: Vec<Value> = result
            .entries
            .into_iter()
            .map(|r| {
                json!({
                    "fullUrl": format!("{}/{}", r.resource_type, r.id),
                    "resource": r.resource,
                })
            })
            .collect();

        Ok(json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": result.total,
            "entry": entries,
        }))
    })
}

fn fhir_patch(ctx: OpContext, args: Vec<Value>) -> otter_runtime::OpResult {
    let fhir_ctx = get_fhir_context(&ctx)?;

    let resource_type = args
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.patch requires resourceType as first argument")
        })?
        .to_string();

    let id = args
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            otter_runtime::JscError::internal("fhir.patch requires id as second argument")
        })?
        .to_string();

    let patch = args.get(2).cloned().ok_or_else(|| {
        otter_runtime::JscError::internal("fhir.patch requires patch object as third argument")
    })?;

    debug!(resource_type = %resource_type, id = %id, "fhir.patch called");

    let storage = fhir_ctx.storage.clone();
    fhir_ctx.handle.block_on(async move {
        // Read current resource
        let current = match storage.read(&resource_type, &id).await {
            Ok(Some(stored)) => stored.resource,
            Ok(None) => {
                return Err(otter_runtime::JscError::internal(format!(
                    "{}/{} not found",
                    resource_type, id
                )))
            }
            Err(e) => {
                warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.patch read failed");
                return Err(otter_runtime::JscError::internal(format!(
                    "Failed to read resource: {}",
                    e
                )));
            }
        };

        // Merge patch into current
        let mut merged = current.clone();
        if let (Some(obj), Some(patch_obj)) = (merged.as_object_mut(), patch.as_object()) {
            for (key, value) in patch_obj {
                obj.insert(key.clone(), value.clone());
            }
        }

        // Update with merged resource
        storage
            .update(&merged, None)
            .await
            .map(|stored| stored.resource)
            .map_err(|e| {
                warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.patch update failed");
                otter_runtime::JscError::internal(format!("Failed to patch resource: {}", e))
            })
    })
}
