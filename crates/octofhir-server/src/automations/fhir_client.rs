//! FHIR client implementation for automation execution.
//!
//! This module provides a `FhirClient` implementation that uses the
//! OctoFHIR storage layer for FHIR operations.

use crate::automations::fhir_api::FhirClient;
use octofhir_storage::{FhirStorage, SearchParams};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

/// FHIR client implementation backed by OctoFHIR storage
pub struct StorageFhirClient {
    storage: Arc<dyn FhirStorage>,
    handle: Handle,
}

impl StorageFhirClient {
    /// Create a new storage-backed FHIR client
    pub fn new(storage: Arc<dyn FhirStorage>, handle: Handle) -> Self {
        Self { storage, handle }
    }
}

impl FhirClient for StorageFhirClient {
    fn create(&self, resource: serde_json::Value) -> Result<serde_json::Value, String> {
        let storage = self.storage.clone();
        self.handle.block_on(async move {
            storage
                .create(&resource)
                .await
                .map(|stored| stored.resource)
                .map_err(|e| format!("Failed to create resource: {}", e))
        })
    }

    fn read(&self, resource_type: &str, id: &str) -> Result<serde_json::Value, String> {
        let storage = self.storage.clone();
        let resource_type = resource_type.to_string();
        let id = id.to_string();

        self.handle.block_on(async move {
            match storage.read(&resource_type, &id).await {
                Ok(Some(stored)) => Ok(stored.resource),
                Ok(None) => Err(format!("{}/{} not found", resource_type, id)),
                Err(e) => Err(format!("Failed to read resource: {}", e)),
            }
        })
    }

    fn update(&self, resource: serde_json::Value) -> Result<serde_json::Value, String> {
        let storage = self.storage.clone();
        self.handle.block_on(async move {
            storage
                .update(&resource, None)
                .await
                .map(|stored| stored.resource)
                .map_err(|e| format!("Failed to update resource: {}", e))
        })
    }

    fn delete(&self, resource_type: &str, id: &str) -> Result<(), String> {
        let storage = self.storage.clone();
        let resource_type = resource_type.to_string();
        let id = id.to_string();

        self.handle.block_on(async move {
            storage
                .delete(&resource_type, &id)
                .await
                .map_err(|e| format!("Failed to delete resource: {}", e))
        })
    }

    fn search(
        &self,
        resource_type: &str,
        params: HashMap<String, String>,
    ) -> Result<serde_json::Value, String> {
        let storage = self.storage.clone();
        let resource_type = resource_type.to_string();

        self.handle.block_on(async move {
            // Convert params to SearchParams
            let mut search_params = SearchParams::new();
            for (key, value) in params {
                search_params.parameters.insert(key, vec![value]);
            }

            let result = storage
                .search(&resource_type, &search_params)
                .await
                .map_err(|e| format!("Failed to search resources: {}", e))?;

            // Convert SearchResult to Bundle JSON
            let entries: Vec<serde_json::Value> = result
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

    fn patch(
        &self,
        resource_type: &str,
        id: &str,
        patch: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let storage = self.storage.clone();
        let resource_type = resource_type.to_string();
        let id = id.to_string();

        self.handle.block_on(async move {
            // Read current resource
            let current = match storage.read(&resource_type, &id).await {
                Ok(Some(stored)) => stored.resource,
                Ok(None) => return Err(format!("{}/{} not found", resource_type, id)),
                Err(e) => return Err(format!("Failed to read resource: {}", e)),
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
                .map_err(|e| format!("Failed to patch resource: {}", e))
        })
    }
}
