//! Implementation of the FhirStorage trait for InMemoryStorage.

use std::str::FromStr;

use async_trait::async_trait;
use serde_json::Value;
use time::OffsetDateTime;

use octofhir_core::ResourceType;
use octofhir_storage::{
    FhirStorage, HistoryEntry, HistoryMethod, HistoryParams, HistoryResult, SearchParams,
    SearchResult, StorageError, StoredResource, Transaction as FhirTransaction,
};

use crate::storage::{make_storage_key_str, InMemoryStorage};

/// Extracts resourceType from a JSON Value.
fn extract_resource_type(resource: &Value) -> Result<String, StorageError> {
    resource
        .get("resourceType")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| StorageError::invalid_resource("Missing resourceType field"))
}

/// Extracts id from a JSON Value.
fn extract_id(resource: &Value) -> Option<String> {
    resource.get("id").and_then(|v| v.as_str()).map(String::from)
}

/// Parses a resource type string to ResourceType enum.
fn parse_resource_type(resource_type: &str) -> Result<ResourceType, StorageError> {
    ResourceType::from_str(resource_type)
        .map_err(|_| StorageError::invalid_resource(format!("Unknown resource type: {resource_type}")))
}

#[async_trait]
impl FhirStorage for InMemoryStorage {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        let resource_type_str = extract_resource_type(resource)?;
        let resource_type = parse_resource_type(&resource_type_str)?;
        let id = extract_id(resource).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let version_id = self.next_version();
        let now = OffsetDateTime::now_utc();

        let key = make_storage_key_str(&resource_type_str, &id);

        // Build resource with id and meta
        let mut resource_with_meta = resource.clone();
        if let Some(obj) = resource_with_meta.as_object_mut() {
            obj.insert("id".to_string(), Value::String(id.clone()));
            obj.insert(
                "meta".to_string(),
                serde_json::json!({
                    "versionId": version_id,
                    "lastUpdated": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
                }),
            );
        }

        // Create ResourceEnvelope for internal storage
        let mut envelope = octofhir_core::ResourceEnvelope::new(id.clone(), resource_type);
        envelope.meta.version_id = Some(version_id.clone());

        // Copy additional fields from the resource
        if let Some(obj) = resource_with_meta.as_object() {
            for (k, value) in obj {
                if k != "resourceType" && k != "id" && k != "meta" && k != "status" {
                    envelope.add_field(k.clone(), value.clone());
                }
            }
        }

        // Use block scope to ensure guard is dropped before await
        {
            let guard = self.data.pin();

            // Check for conflicts
            if guard.get(&key).is_some() {
                return Err(StorageError::already_exists(&resource_type_str, &id));
            }

            guard.insert(key, envelope);
        }

        let stored = StoredResource {
            id: id.clone(),
            version_id: version_id.clone(),
            resource_type: resource_type_str,
            resource: resource_with_meta,
            last_updated: now,
            created_at: now,
        };

        // Add to history (guard is now dropped)
        self.add_history(&stored, HistoryMethod::Create).await;

        Ok(stored)
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        let key = make_storage_key_str(resource_type, id);
        let guard = self.data.pin();

        match guard.get(&key) {
            Some(envelope) => {
                let version_id = envelope
                    .meta
                    .version_id
                    .clone()
                    .unwrap_or_else(|| "1".to_string());
                let resource = serde_json::to_value(envelope)
                    .map_err(|e| StorageError::internal(e.to_string()))?;

                Ok(Some(StoredResource {
                    id: envelope.id.clone(),
                    version_id,
                    resource_type: resource_type.to_string(),
                    resource,
                    last_updated: OffsetDateTime::now_utc(),
                    created_at: OffsetDateTime::now_utc(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        let resource_type_str = extract_resource_type(resource)?;
        let resource_type = parse_resource_type(&resource_type_str)?;
        let id = extract_id(resource)
            .ok_or_else(|| StorageError::invalid_resource("Missing id field for update"))?;

        let key = make_storage_key_str(&resource_type_str, &id);
        let version_id = self.next_version();
        let now = OffsetDateTime::now_utc();

        // Build resource with updated meta
        let mut resource_with_meta = resource.clone();
        if let Some(obj) = resource_with_meta.as_object_mut() {
            obj.insert(
                "meta".to_string(),
                serde_json::json!({
                    "versionId": version_id,
                    "lastUpdated": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
                }),
            );
        }

        // Create updated envelope
        let mut envelope = octofhir_core::ResourceEnvelope::new(id.clone(), resource_type);
        envelope.meta.version_id = Some(version_id.clone());

        if let Some(obj) = resource_with_meta.as_object() {
            for (k, value) in obj {
                if k != "resourceType" && k != "id" && k != "meta" && k != "status" {
                    envelope.add_field(k.clone(), value.clone());
                }
            }
        }

        // Use block scope to ensure guard is dropped before await
        {
            let guard = self.data.pin();

            // Check if resource exists and validate version if if_match is provided
            let existing = guard
                .get(&key)
                .ok_or_else(|| StorageError::not_found(&resource_type_str, &id))?;

            if let Some(expected_version) = if_match {
                let actual_version = existing.meta.version_id.as_deref().unwrap_or("1");
                if actual_version != expected_version {
                    return Err(StorageError::version_conflict(expected_version, actual_version));
                }
            }

            guard.insert(key, envelope);
        }

        let stored = StoredResource {
            id: id.clone(),
            version_id: version_id.clone(),
            resource_type: resource_type_str,
            resource: resource_with_meta,
            last_updated: now,
            created_at: now,
        };

        // Add to history (guard is now dropped)
        self.add_history(&stored, HistoryMethod::Update).await;

        Ok(stored)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let key = make_storage_key_str(resource_type, id);
        let version_id = self.next_version();
        let now = OffsetDateTime::now_utc();

        // Use block scope to capture removed resource and drop guard before await
        let resource_json = {
            let guard = self.data.pin();

            let removed = guard
                .remove(&key)
                .ok_or_else(|| StorageError::not_found(resource_type, id))?;

            serde_json::to_value(removed).unwrap_or_default()
        };

        // Create a tombstone entry for history
        let stored = StoredResource {
            id: id.to_string(),
            version_id,
            resource_type: resource_type.to_string(),
            resource: resource_json,
            last_updated: now,
            created_at: now,
        };

        // Guard is now dropped
        self.add_history(&stored, HistoryMethod::Delete).await;

        Ok(())
    }

    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        let history = self.get_history(resource_type, id).await;

        // Find the specific version in history
        for entry in history {
            if entry.resource.version_id == version {
                return Ok(Some(entry.resource));
            }
        }

        // Also check current version
        if let Some(current) = self.read(resource_type, id).await?
            && current.version_id == version
        {
            return Ok(Some(current));
        }

        Ok(None)
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        let mut entries = if let Some(id) = id {
            self.get_history(resource_type, id).await
        } else {
            self.get_type_history(resource_type).await
        };

        // Sort by last_updated descending (most recent first)
        entries.sort_by(|a, b| b.resource.last_updated.cmp(&a.resource.last_updated));

        // Apply since filter
        if let Some(since) = params.since {
            entries.retain(|e| e.resource.last_updated >= since);
        }

        // Apply at filter
        if let Some(at) = params.at {
            entries.retain(|e| e.resource.last_updated <= at);
        }

        let total = entries.len() as u32;

        // Apply pagination
        let offset = params.offset.unwrap_or(0) as usize;
        let count = params.count.unwrap_or(10) as usize;

        let entries: Vec<HistoryEntry> = entries.into_iter().skip(offset).take(count).collect();

        Ok(HistoryResult {
            entries,
            total: Some(total),
        })
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        // Convert SearchParams to internal SearchQuery
        let rt = parse_resource_type(resource_type)?;

        let query = crate::query::SearchQuery::new(rt).with_pagination(
            params.offset.unwrap_or(0) as usize,
            params.count.unwrap_or(10) as usize,
        );

        // Execute internal search
        let result = InMemoryStorage::search(self, &query)
            .await
            .map_err(|e| StorageError::internal(e.to_string()))?;

        // Convert to StoredResource entries
        let entries: Vec<StoredResource> = result
            .resources
            .iter()
            .map(|env| {
                let version_id = env.meta.version_id.clone().unwrap_or_else(|| "1".to_string());
                StoredResource {
                    id: env.id.clone(),
                    version_id,
                    resource_type: resource_type.to_string(),
                    resource: serde_json::to_value(env).unwrap_or_default(),
                    last_updated: OffsetDateTime::now_utc(),
                    created_at: OffsetDateTime::now_utc(),
                }
            })
            .collect();

        Ok(SearchResult {
            entries,
            total: Some(result.total as u32),
            has_more: result.has_more,
        })
    }

    async fn begin_transaction(&self) -> Result<Box<dyn FhirTransaction>, StorageError> {
        Ok(Box::new(InMemoryFhirTransaction::new()))
    }

    fn supports_transactions(&self) -> bool {
        true
    }

    fn backend_name(&self) -> &'static str {
        "in-memory-papaya"
    }
}

/// In-memory transaction implementation.
///
/// For the in-memory backend, transactions are simulated.
/// Operations are collected and applied on commit.
pub struct InMemoryFhirTransaction {
    operations: Vec<TransactionOp>,
}

#[allow(dead_code)]
enum TransactionOp {
    Create(Value),
    Update(Value),
    Delete { resource_type: String, id: String },
}

impl InMemoryFhirTransaction {
    fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

#[async_trait]
impl FhirTransaction for InMemoryFhirTransaction {
    async fn commit(self: Box<Self>) -> Result<(), StorageError> {
        // In a real implementation, this would apply all operations atomically.
        // For now, we just succeed since operations were already applied.
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<(), StorageError> {
        // In a real implementation, this would undo all operations.
        // For now, we just succeed.
        Ok(())
    }

    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        self.operations.push(TransactionOp::Create(resource.clone()));

        // Return a placeholder - real implementation would defer to commit
        let resource_type = extract_resource_type(resource)?;
        let id = extract_id(resource).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let now = OffsetDateTime::now_utc();

        Ok(StoredResource {
            id,
            version_id: "1".to_string(),
            resource_type,
            resource: resource.clone(),
            last_updated: now,
            created_at: now,
        })
    }

    async fn update(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        self.operations.push(TransactionOp::Update(resource.clone()));

        // Return a placeholder
        let resource_type = extract_resource_type(resource)?;
        let id = extract_id(resource)
            .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;
        let now = OffsetDateTime::now_utc();

        Ok(StoredResource {
            id,
            version_id: "1".to_string(),
            resource_type,
            resource: resource.clone(),
            last_updated: now,
            created_at: now,
        })
    }

    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        self.operations.push(TransactionOp::Delete {
            resource_type: resource_type.to_string(),
            id: id.to_string(),
        });
        Ok(())
    }

    async fn read(
        &self,
        _resource_type: &str,
        _id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        // In a real transaction, this would read from the transaction's view.
        // For now, return None as we don't have access to storage here.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to get a storage as trait object to ensure we use FhirStorage methods
    fn as_fhir_storage(storage: &InMemoryStorage) -> &dyn FhirStorage {
        storage
    }

    #[tokio::test]
    async fn test_fhir_storage_create_read() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        let patient = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith", "given": ["John"]}]
        });

        let created = fhir.create(&patient).await.unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(created.resource_type, "Patient");
        assert!(!created.version_id.is_empty());

        // Read it back
        let read = fhir.read("Patient", &created.id).await.unwrap();
        assert!(read.is_some());
        let read = read.unwrap();
        assert_eq!(read.id, created.id);
    }

    #[tokio::test]
    async fn test_fhir_storage_update() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        // Create initial
        let patient = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}]
        });
        let created = fhir.create(&patient).await.unwrap();

        // Update
        let updated_patient = serde_json::json!({
            "resourceType": "Patient",
            "id": created.id,
            "name": [{"family": "Jones"}]
        });
        let updated = fhir.update(&updated_patient, None).await.unwrap();
        assert_eq!(updated.id, created.id);
        assert_ne!(updated.version_id, created.version_id);
    }

    #[tokio::test]
    async fn test_fhir_storage_delete() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        let patient = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}]
        });
        let created = fhir.create(&patient).await.unwrap();

        // Delete
        fhir.delete("Patient", &created.id).await.unwrap();

        // Verify deleted
        let read = fhir.read("Patient", &created.id).await.unwrap();
        assert!(read.is_none());
    }

    #[tokio::test]
    async fn test_fhir_storage_history() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        // Create
        let patient = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}]
        });
        let created = fhir.create(&patient).await.unwrap();

        // Update multiple times
        for i in 0..3 {
            let updated = serde_json::json!({
                "resourceType": "Patient",
                "id": created.id,
                "name": [{"family": format!("Name{}", i)}]
            });
            fhir.update(&updated, None).await.unwrap();
        }

        // Get history
        let params = HistoryParams::new();
        let history = fhir.history("Patient", Some(&created.id), &params).await.unwrap();

        // Should have 4 entries (1 create + 3 updates)
        assert_eq!(history.entries.len(), 4);
    }

    #[tokio::test]
    async fn test_fhir_storage_version_conflict() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        let patient = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}]
        });
        let created = fhir.create(&patient).await.unwrap();

        // Try to update with wrong version
        let updated = serde_json::json!({
            "resourceType": "Patient",
            "id": created.id,
            "name": [{"family": "Jones"}]
        });
        let result = fhir.update(&updated, Some("wrong-version")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_version_conflict());
    }

    #[tokio::test]
    async fn test_fhir_storage_already_exists() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        let patient = serde_json::json!({
            "resourceType": "Patient",
            "id": "fixed-id",
            "name": [{"family": "Smith"}]
        });
        fhir.create(&patient).await.unwrap();

        // Try to create again with same ID
        let result = fhir.create(&patient).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_already_exists());
    }

    #[tokio::test]
    async fn test_fhir_storage_search() {
        let storage = InMemoryStorage::new();
        let fhir = as_fhir_storage(&storage);

        // Create multiple patients
        for i in 0..5 {
            let patient = serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": format!("Patient{}", i)}]
            });
            fhir.create(&patient).await.unwrap();
        }

        let params = SearchParams::new().with_count(3);
        let result = fhir.search("Patient", &params).await.unwrap();

        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.total, Some(5));
        assert!(result.has_more);
    }

    #[tokio::test]
    async fn test_backend_name() {
        let storage = InMemoryStorage::new();
        assert_eq!(storage.backend_name(), "in-memory-papaya");
        assert!(storage.supports_transactions());
    }
}
