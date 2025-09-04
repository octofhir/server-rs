use crate::{FhirDateTime, ResourceType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResourceStatus {
    #[default]
    Active,
    Inactive,
    Draft,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceMeta {
    #[serde(rename = "lastUpdated")]
    pub last_updated: FhirDateTime,
    #[serde(rename = "versionId", skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub profile: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub security: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tag: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl ResourceMeta {
    pub fn new() -> Self {
        Self {
            last_updated: crate::time::now_utc(),
            version_id: None,
            profile: Vec::new(),
            security: Vec::new(),
            tag: Vec::new(),
            source: None,
        }
    }

    pub fn with_version_id(mut self, version_id: String) -> Self {
        self.version_id = Some(version_id);
        self
    }

    pub fn with_profile(mut self, profile: Vec<String>) -> Self {
        self.profile = profile;
        self
    }

    pub fn update_timestamp(&mut self) {
        self.last_updated = crate::time::now_utc();
    }
}

impl Default for ResourceMeta {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEnvelope {
    pub id: String,
    #[serde(rename = "resourceType")]
    pub resource_type: ResourceType,
    pub status: ResourceStatus,
    pub meta: ResourceMeta,
    #[serde(flatten)]
    pub data: HashMap<String, Value>,
}

impl ResourceEnvelope {
    pub fn new(id: String, resource_type: ResourceType) -> Self {
        Self {
            id,
            resource_type,
            status: ResourceStatus::default(),
            meta: ResourceMeta::new(),
            data: HashMap::new(),
        }
    }

    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_meta(mut self, meta: ResourceMeta) -> Self {
        self.meta = meta;
        self
    }

    pub fn with_data(mut self, data: HashMap<String, Value>) -> Self {
        self.data = data;
        self
    }

    pub fn add_field(&mut self, key: String, value: Value) {
        self.data.insert(key, value);
    }

    pub fn get_field(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn remove_field(&mut self, key: &str) -> Option<Value> {
        self.data.remove(key)
    }

    pub fn update_meta(&mut self) {
        self.meta.update_timestamp();
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, ResourceStatus::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resource_status_default() {
        let status = ResourceStatus::default();
        assert_eq!(status, ResourceStatus::Active);
    }

    #[test]
    fn test_resource_status_serialization() {
        let status = ResourceStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"Active\"");

        let status = ResourceStatus::Inactive;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"Inactive\"");
    }

    #[test]
    fn test_resource_status_deserialization() {
        let status: ResourceStatus = serde_json::from_str("\"Active\"").unwrap();
        assert_eq!(status, ResourceStatus::Active);

        let status: ResourceStatus = serde_json::from_str("\"Draft\"").unwrap();
        assert_eq!(status, ResourceStatus::Draft);
    }

    #[test]
    fn test_resource_meta_new() {
        let meta = ResourceMeta::new();
        assert!(meta.version_id.is_none());
        assert!(meta.profile.is_empty());
        assert!(meta.security.is_empty());
        assert!(meta.tag.is_empty());
        assert!(meta.source.is_none());
    }

    #[test]
    fn test_resource_meta_default() {
        let meta = ResourceMeta::default();
        assert!(meta.version_id.is_none());
        assert!(meta.profile.is_empty());
    }

    #[test]
    fn test_resource_meta_with_version_id() {
        let meta = ResourceMeta::new().with_version_id("v1.0".to_string());
        assert_eq!(meta.version_id, Some("v1.0".to_string()));
    }

    #[test]
    fn test_resource_meta_with_profile() {
        let profiles = vec!["http://example.com/profile1".to_string()];
        let meta = ResourceMeta::new().with_profile(profiles.clone());
        assert_eq!(meta.profile, profiles);
    }

    #[test]
    fn test_resource_meta_update_timestamp() {
        let mut meta = ResourceMeta::new();
        let original_timestamp = meta.last_updated.clone();

        std::thread::sleep(std::time::Duration::from_millis(1));
        meta.update_timestamp();

        assert!(meta.last_updated > original_timestamp);
    }

    #[test]
    fn test_resource_meta_serialization() {
        let meta = ResourceMeta::new().with_version_id("v1".to_string());
        let json = serde_json::to_value(&meta).unwrap();

        assert!(json["lastUpdated"].is_string());
        assert_eq!(json["versionId"], "v1");

        assert!(json.get("profile").is_none() || json["profile"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_resource_meta_deserialization() {
        let json = json!({
            "lastUpdated": "2023-05-15T14:30:00Z",
            "versionId": "v1"
        });

        let meta: ResourceMeta = serde_json::from_value(json).unwrap();
        assert_eq!(meta.version_id, Some("v1".to_string()));
    }

    #[test]
    fn test_resource_envelope_new() {
        let envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient);

        assert_eq!(envelope.id, "patient-123");
        assert_eq!(envelope.resource_type, ResourceType::Patient);
        assert_eq!(envelope.status, ResourceStatus::Active);
        assert!(envelope.data.is_empty());
    }

    #[test]
    fn test_resource_envelope_with_status() {
        let envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient)
            .with_status(ResourceStatus::Draft);

        assert_eq!(envelope.status, ResourceStatus::Draft);
    }

    #[test]
    fn test_resource_envelope_with_meta() {
        let meta = ResourceMeta::new().with_version_id("v2".to_string());
        let envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient)
            .with_meta(meta.clone());

        assert_eq!(envelope.meta, meta);
    }

    #[test]
    fn test_resource_envelope_with_data() {
        let mut data = HashMap::new();
        data.insert("name".to_string(), json!("John Doe"));

        let envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient)
            .with_data(data.clone());

        assert_eq!(envelope.data, data);
    }

    #[test]
    fn test_resource_envelope_field_operations() {
        let mut envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient);

        envelope.add_field("name".to_string(), json!("John Doe"));
        assert_eq!(envelope.get_field("name"), Some(&json!("John Doe")));

        envelope.add_field("age".to_string(), json!(30));
        assert_eq!(envelope.get_field("age"), Some(&json!(30)));

        let removed = envelope.remove_field("name");
        assert_eq!(removed, Some(json!("John Doe")));
        assert!(envelope.get_field("name").is_none());
    }

    #[test]
    fn test_resource_envelope_update_meta() {
        let mut envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient);
        let original_timestamp = envelope.meta.last_updated.clone();

        std::thread::sleep(std::time::Duration::from_millis(1));
        envelope.update_meta();

        assert!(envelope.meta.last_updated > original_timestamp);
    }

    #[test]
    fn test_resource_envelope_is_active() {
        let active_envelope =
            ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient);
        assert!(active_envelope.is_active());

        let inactive_envelope =
            ResourceEnvelope::new("patient-456".to_string(), ResourceType::Patient)
                .with_status(ResourceStatus::Inactive);
        assert!(!inactive_envelope.is_active());
    }

    #[test]
    fn test_resource_envelope_serialization() {
        let mut envelope = ResourceEnvelope::new("patient-123".to_string(), ResourceType::Patient);
        envelope.add_field("name".to_string(), json!("John Doe"));
        envelope.add_field("birthDate".to_string(), json!("1990-01-01"));

        let json = serde_json::to_value(&envelope).unwrap();

        assert_eq!(json["id"], "patient-123");
        assert_eq!(json["resourceType"], "Patient");
        assert_eq!(json["status"], "Active");
        assert_eq!(json["name"], "John Doe");
        assert_eq!(json["birthDate"], "1990-01-01");
        assert!(json["meta"]["lastUpdated"].is_string());
    }

    #[test]
    fn test_resource_envelope_deserialization() {
        let json = json!({
            "id": "patient-789",
            "resourceType": "Patient",
            "status": "Active",
            "meta": {
                "lastUpdated": "2023-05-15T14:30:00Z",
                "versionId": "v1"
            },
            "name": "Jane Doe",
            "gender": "female"
        });

        let envelope: ResourceEnvelope = serde_json::from_value(json).unwrap();

        assert_eq!(envelope.id, "patient-789");
        assert_eq!(envelope.resource_type, ResourceType::Patient);
        assert_eq!(envelope.status, ResourceStatus::Active);
        assert_eq!(envelope.meta.version_id, Some("v1".to_string()));
        assert_eq!(envelope.get_field("name"), Some(&json!("Jane Doe")));
        assert_eq!(envelope.get_field("gender"), Some(&json!("female")));
    }

    #[test]
    fn test_resource_envelope_roundtrip() {
        let original = ResourceEnvelope::new("test-123".to_string(), ResourceType::Organization)
            .with_status(ResourceStatus::Draft);

        let json = serde_json::to_value(&original).unwrap();
        let deserialized: ResourceEnvelope = serde_json::from_value(json).unwrap();

        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.resource_type, deserialized.resource_type);
        assert_eq!(original.status, deserialized.status);
    }

    #[test]
    fn test_resource_envelope_equality() {
        let timestamp = crate::time::now_utc();
        let meta = ResourceMeta {
            last_updated: timestamp.clone(),
            version_id: None,
            profile: Vec::new(),
            security: Vec::new(),
            tag: Vec::new(),
            source: None,
        };

        let envelope1 = ResourceEnvelope::new("test-123".to_string(), ResourceType::Patient)
            .with_meta(meta.clone());
        let envelope2 =
            ResourceEnvelope::new("test-123".to_string(), ResourceType::Patient).with_meta(meta);
        let envelope3 = ResourceEnvelope::new("test-456".to_string(), ResourceType::Patient);

        assert_eq!(envelope1, envelope2);
        assert_ne!(envelope1, envelope3);
    }

    #[test]
    fn test_resource_envelope_debug() {
        let envelope = ResourceEnvelope::new("debug-test".to_string(), ResourceType::Observation);
        let debug_str = format!("{envelope:?}");

        assert!(debug_str.contains("ResourceEnvelope"));
        assert!(debug_str.contains("debug-test"));
        assert!(debug_str.contains("Observation"));
    }

    #[test]
    fn test_resource_envelope_empty_optional_fields() {
        let envelope = ResourceEnvelope::new("empty-test".to_string(), ResourceType::Patient);
        let json = serde_json::to_value(&envelope).unwrap();

        assert!(json.get("profile").is_none() || json["profile"].as_array().unwrap().is_empty());
        assert!(json.get("security").is_none() || json["security"].as_array().unwrap().is_empty());
        assert!(json.get("tag").is_none() || json["tag"].as_array().unwrap().is_empty());
        assert!(json.get("source").is_none());
    }

    #[test]
    fn test_all_resource_status_variants() {
        let statuses = [
            ResourceStatus::Active,
            ResourceStatus::Inactive,
            ResourceStatus::Draft,
            ResourceStatus::Unknown,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let deserialized: ResourceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn test_resource_envelope_with_complex_data() {
        let mut envelope = ResourceEnvelope::new("complex-123".to_string(), ResourceType::Patient);

        envelope.add_field(
            "name".to_string(),
            json!([{
                "use": "official",
                "given": ["John"],
                "family": "Doe"
            }]),
        );

        envelope.add_field(
            "address".to_string(),
            json!([{
                "use": "home",
                "line": ["123 Main St"],
                "city": "Anytown",
                "state": "CA",
                "postalCode": "90210"
            }]),
        );

        let name_array = envelope.get_field("name").unwrap().as_array().unwrap();
        assert_eq!(name_array.len(), 1);
        assert_eq!(name_array[0]["family"], "Doe");
    }
}
