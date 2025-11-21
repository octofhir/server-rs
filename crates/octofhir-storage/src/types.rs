//! Storage types for the FHIR storage abstraction layer.
//!
//! This module defines all data types used by the storage traits.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use time::OffsetDateTime;

/// A FHIR resource as stored in the storage backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResource {
    /// The resource ID.
    pub id: String,
    /// The version ID of this specific version.
    pub version_id: String,
    /// The FHIR resource type (e.g., "Patient", "Observation").
    pub resource_type: String,
    /// The full resource content as JSON.
    pub resource: Value,
    /// When this version was last updated.
    #[serde(with = "time::serde::rfc3339")]
    pub last_updated: OffsetDateTime,
    /// When the resource was originally created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl StoredResource {
    /// Creates a new `StoredResource`.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        version_id: impl Into<String>,
        resource_type: impl Into<String>,
        resource: Value,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: id.into(),
            version_id: version_id.into(),
            resource_type: resource_type.into(),
            resource,
            last_updated: now,
            created_at: now,
        }
    }

    /// Creates a new version of this resource with updated content.
    #[must_use]
    pub fn new_version(&self, version_id: impl Into<String>, resource: Value) -> Self {
        Self {
            id: self.id.clone(),
            version_id: version_id.into(),
            resource_type: self.resource_type.clone(),
            resource,
            last_updated: OffsetDateTime::now_utc(),
            created_at: self.created_at,
        }
    }
}

/// Result of a search operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matching resources.
    pub entries: Vec<StoredResource>,
    /// Total count of matching resources, if requested and available.
    pub total: Option<u32>,
    /// Whether there are more results available beyond this page.
    pub has_more: bool,
}

impl SearchResult {
    /// Creates a new empty `SearchResult`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a new `SearchResult` with entries.
    #[must_use]
    pub fn with_entries(entries: Vec<StoredResource>) -> Self {
        Self {
            entries,
            total: None,
            has_more: false,
        }
    }

    /// Sets the total count.
    #[must_use]
    pub fn with_total(mut self, total: u32) -> Self {
        self.total = Some(total);
        self
    }

    /// Sets the has_more flag.
    #[must_use]
    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.has_more = has_more;
        self
    }

    /// Returns the number of entries in this result.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Result of a history operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryResult {
    /// The history entries.
    pub entries: Vec<HistoryEntry>,
    /// Total count of history entries, if available.
    pub total: Option<u32>,
}

impl HistoryResult {
    /// Creates a new empty `HistoryResult`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a new `HistoryResult` with entries.
    #[must_use]
    pub fn with_entries(entries: Vec<HistoryEntry>) -> Self {
        Self {
            entries,
            total: None,
        }
    }

    /// Returns the number of entries in this result.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A single entry in a resource's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The resource at this point in history.
    pub resource: StoredResource,
    /// The operation that created this history entry.
    pub method: HistoryMethod,
}

impl HistoryEntry {
    /// Creates a new `HistoryEntry`.
    #[must_use]
    pub fn new(resource: StoredResource, method: HistoryMethod) -> Self {
        Self { resource, method }
    }
}

/// The HTTP method/operation that created a history entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryMethod {
    /// Resource was created (POST).
    Create,
    /// Resource was updated (PUT).
    Update,
    /// Resource was deleted (DELETE).
    Delete,
}

impl std::fmt::Display for HistoryMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => write!(f, "POST"),
            Self::Update => write!(f, "PUT"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

/// Parameters for a history query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryParams {
    /// Only include entries from after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub since: Option<OffsetDateTime>,
    /// Only include entries from before this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub at: Option<OffsetDateTime>,
    /// Maximum number of entries to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Number of entries to skip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

impl HistoryParams {
    /// Creates new default `HistoryParams`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the since parameter.
    #[must_use]
    pub fn since(mut self, since: OffsetDateTime) -> Self {
        self.since = Some(since);
        self
    }

    /// Sets the at parameter.
    #[must_use]
    pub fn at(mut self, at: OffsetDateTime) -> Self {
        self.at = Some(at);
        self
    }

    /// Sets the count parameter.
    #[must_use]
    pub fn count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }

    /// Sets the offset parameter.
    #[must_use]
    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Parameters for a search query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchParams {
    /// Search parameters as key-value pairs.
    /// Multiple values for the same key represent OR conditions.
    pub parameters: HashMap<String, Vec<String>>,
    /// Maximum number of results to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Number of results to skip for pagination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// Sort parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Vec<SortParam>>,
    /// How to calculate the total count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<TotalMode>,
}

impl SearchParams {
    /// Creates new empty `SearchParams`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a search parameter.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters
            .entry(key.into())
            .or_default()
            .push(value.into());
        self
    }

    /// Sets the count parameter.
    #[must_use]
    pub fn with_count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }

    /// Sets the offset parameter.
    #[must_use]
    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Adds a sort parameter.
    #[must_use]
    pub fn with_sort(mut self, field: impl Into<String>, descending: bool) -> Self {
        self.sort
            .get_or_insert_with(Vec::new)
            .push(SortParam::new(field, descending));
        self
    }

    /// Sets the total mode.
    #[must_use]
    pub fn with_total(mut self, mode: TotalMode) -> Self {
        self.total = Some(mode);
        self
    }

    /// Returns true if this search has no parameters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty()
    }
}

/// A sort parameter for search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortParam {
    /// The field to sort by.
    pub field: String,
    /// Whether to sort in descending order.
    pub descending: bool,
}

impl SortParam {
    /// Creates a new `SortParam`.
    #[must_use]
    pub fn new(field: impl Into<String>, descending: bool) -> Self {
        Self {
            field: field.into(),
            descending,
        }
    }

    /// Creates an ascending sort parameter.
    #[must_use]
    pub fn asc(field: impl Into<String>) -> Self {
        Self::new(field, false)
    }

    /// Creates a descending sort parameter.
    #[must_use]
    pub fn desc(field: impl Into<String>) -> Self {
        Self::new(field, true)
    }
}

/// How to calculate the total count in search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TotalMode {
    /// Return an accurate count (may be expensive).
    Accurate,
    /// Return an estimated count (faster but approximate).
    Estimate,
    /// Do not return a count.
    #[default]
    None,
}

impl std::fmt::Display for TotalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accurate => write!(f, "accurate"),
            Self::Estimate => write!(f, "estimate"),
            Self::None => write!(f, "none"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stored_resource_serialization() {
        let resource = StoredResource::new(
            "123",
            "1",
            "Patient",
            serde_json::json!({"resourceType": "Patient", "id": "123"}),
        );

        let json = serde_json::to_string(&resource).expect("serialization failed");
        let deserialized: StoredResource =
            serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(resource.id, deserialized.id);
        assert_eq!(resource.version_id, deserialized.version_id);
        assert_eq!(resource.resource_type, deserialized.resource_type);
    }

    #[test]
    fn test_search_params_builder() {
        let params = SearchParams::new()
            .with_param("name", "John")
            .with_param("name", "Jane")
            .with_param("birthdate", "1990-01-01")
            .with_count(10)
            .with_offset(20)
            .with_sort("name", false)
            .with_total(TotalMode::Accurate);

        assert_eq!(params.parameters.get("name").unwrap().len(), 2);
        assert_eq!(params.count, Some(10));
        assert_eq!(params.offset, Some(20));
        assert_eq!(params.sort.as_ref().unwrap().len(), 1);
        assert_eq!(params.total, Some(TotalMode::Accurate));
    }

    #[test]
    fn test_search_result() {
        let result = SearchResult::empty()
            .with_total(100)
            .with_has_more(true);

        assert!(result.is_empty());
        assert_eq!(result.total, Some(100));
        assert!(result.has_more);
    }

    #[test]
    fn test_history_method_display() {
        assert_eq!(HistoryMethod::Create.to_string(), "POST");
        assert_eq!(HistoryMethod::Update.to_string(), "PUT");
        assert_eq!(HistoryMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_total_mode_serialization() {
        let json = serde_json::to_string(&TotalMode::Accurate).unwrap();
        assert_eq!(json, "\"accurate\"");

        let mode: TotalMode = serde_json::from_str("\"estimate\"").unwrap();
        assert_eq!(mode, TotalMode::Estimate);
    }
}
