//! Storage types for the FHIR storage abstraction layer.
//!
//! This module defines all data types used by the storage traits.

use crate::StorageError;
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

/// Exact before/after snapshots of one atomic update.
#[derive(Debug, Clone)]
pub struct UpdatedResource {
    pub stored: StoredResource,
    pub previous: StoredResource,
}

/// Raw-JSON counterpart of [`UpdatedResource`]: the committed body is kept as
/// text for the response, the previous body is parsed only for events.
#[derive(Debug, Clone)]
pub struct RawUpdatedResource {
    pub stored: RawStoredResource,
    pub previous: StoredResource,
}

/// A FHIR resource stored with raw JSON string for zero-copy serialization.
///
/// This is an optimized version of `StoredResource` that keeps the resource
/// as a raw JSON string, avoiding the overhead of parsing JSONB into a full
/// JSON tree when the resource will just be serialized again in the response.
#[derive(Debug, Clone)]
pub struct RawStoredResource {
    /// The resource ID.
    pub id: String,
    /// The version ID of this specific version.
    pub version_id: String,
    /// The FHIR resource type (e.g., "Patient", "Observation").
    pub resource_type: String,
    /// The full resource content as raw JSON string.
    pub resource_json: String,
    /// When this version was last updated.
    pub last_updated: OffsetDateTime,
    /// When the resource was originally created.
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

/// A trusted proposed mutation used only by read-only transaction graph search.
/// `None` removes an identity; `Some` replaces its body. Never a wire input.
#[derive(Debug, Clone, Serialize)]
pub struct ProposedResource {
    pub resource_type: String,
    pub id: String,
    pub resource: Option<Value>,
}

/// Server-reserved metadata shared by a proposed write graph and its commit.
/// Versions are opaque to callers; a rolled-back reservation may leave a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionWriteMetadata {
    pub version_id: String,
    pub last_updated: String,
}

/// Exact live snapshot removed by a mutation and its separate deletion revision.
#[derive(Debug, Clone)]
pub struct DeletedResource {
    pub previous: StoredResource,
    pub version_id: String,
    pub last_updated: OffsetDateTime,
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

/// A trusted snapshot predicate, evaluated on the exact stored JSON before
/// result pagination or following an inline reference.
pub type RawSnapshotPredicate<'a> =
    dyn Fn(&RawStoredResource) -> Result<bool, crate::StorageError> + Send + Sync + 'a;

/// Application policies which cannot be expressed as native search predicates.
/// SQL grants remain part of SearchParams and are always applied first.
#[derive(Clone, Copy, Default)]
pub struct RawSearchAuthorization<'a> {
    pub main: Option<&'a RawSnapshotPredicate<'a>>,
    pub included: Option<&'a RawSnapshotPredicate<'a>>,
}

/// Result of a search operation with raw JSON resources.
///
/// This is an optimized version of `SearchResult` that keeps resources
/// as raw JSON strings for zero-copy serialization in HTTP responses.
#[derive(Debug, Clone, Default)]
pub struct RawSearchResult {
    /// The matching resources with raw JSON.
    pub entries: Vec<RawStoredResource>,
    /// Included resources from _include/_revinclude (different resource types).
    pub included: Vec<RawStoredResource>,
    /// Total count of matching resources, if requested and available.
    pub total: Option<u32>,
    /// Whether there are more results available beyond this page.
    pub has_more: bool,
    /// Warning messages to include in the response (e.g., unknown parameters ignored).
    pub warnings: Vec<String>,
    /// Parameter names actually ignored by execution, for honest self links.
    pub ignored_parameters: Vec<String>,
    /// Optional safe search debug payload, populated only by explicit debug-gated callers.
    pub debug: Option<RawSearchDebug>,
}

/// Safe search debug payload for internal/debug responses.
#[derive(Debug, Clone, Default)]
pub struct RawSearchDebug {
    /// Redacted SQL shape. Bind values are never included.
    pub sql_shape: Option<String>,
    /// FHIR-native search plan/debug metadata.
    pub plan: Option<Value>,
    /// PostgreSQL EXPLAIN JSON output when requested.
    pub explain: Option<Value>,
    /// Whether EXPLAIN ANALYZE was requested.
    pub analyze: bool,
    /// SQL builder elapsed time in milliseconds.
    pub build_elapsed_ms: Option<f64>,
    /// Database execution elapsed time in milliseconds.
    pub db_execute_elapsed_ms: Option<f64>,
}

impl RawSearchResult {
    /// Creates a new empty `RawSearchResult`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
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

/// Raw history entry with resource as raw JSON string (zero-copy path).
#[derive(Debug, Clone)]
pub struct RawHistoryEntry {
    /// The resource at this point in history (raw JSON).
    pub resource: RawStoredResource,
    /// The operation that created this history entry.
    pub method: HistoryMethod,
}

/// Raw history result for optimized serialization.
#[derive(Debug, Clone, Default)]
pub struct RawHistoryResult {
    /// The history entries with raw JSON resources.
    pub entries: Vec<RawHistoryEntry>,
    /// Total count of history entries, if available.
    pub total: Option<u32>,
}

/// Parameters for a history query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryParams {
    /// Only include entries from after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub since: Option<OffsetDateTime>,
    /// Include versions current at this time (or overlapping `at_end`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub at: Option<OffsetDateTime>,
    /// Exclusive upper bound when `_at` specifies a date/time period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub at_end: Option<OffsetDateTime>,
    /// Maximum number of entries to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Number of entries to skip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// How to calculate the total count. When unset, no count is computed and
    /// `Bundle.total` is omitted — avoids a full scan of the current + history
    /// rows on every history request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<TotalMode>,
}

impl HistoryParams {
    /// Creates new default `HistoryParams`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total mode.
    #[must_use]
    pub fn total(mut self, mode: TotalMode) -> Self {
        self.total = Some(mode);
        self
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
///
/// Trusted server-side constraints are applied before pagination and counts.
/// They cannot be supplied through serialized HTTP search parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchParams {
    /// Search parameters as key-value pairs.
    /// Repeated values for the same key are AND conditions; comma-separated
    /// alternatives within one value are OR conditions.
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
    /// Additional row visibility constraints supplied by the authorization layer.
    #[serde(skip)]
    pub constraint: Option<SearchConstraint>,
    /// Trusted constraints for each primary type in an aggregate search.
    /// When present, types missing from this map are denied.
    #[serde(skip)]
    pub type_constraints: Option<HashMap<String, SearchConstraint>>,
    /// Per-type visibility for inline search requests; absent types are denied.
    #[serde(skip)]
    pub included_visibility: Option<IncludeVisibility>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct IncludeLimits {
    pub max_resources: usize,
    pub max_bytes: usize,
    pub max_depth: usize,
}

impl Default for IncludeLimits {
    fn default() -> Self {
        Self {
            max_resources: 1000,
            max_bytes: 16 * 1024 * 1024,
            max_depth: 100,
        }
    }
}

impl IncludeLimits {
    pub fn check(self, resources: usize, bytes: usize) -> Result<(), StorageError> {
        if resources > self.max_resources || bytes > self.max_bytes {
            return Err(StorageError::invalid_resource(
                "Include expansion exceeds configured resource or byte budget",
            ));
        }
        Ok(())
    }
}

/// Trusted authorization of inline searches. It is never deserialized from a client query.
#[derive(Debug, Clone, Default)]
pub struct IncludeVisibility {
    pub constraints: HashMap<String, SearchConstraint>,
    /// Local FHIR base for resolving references without aliasing foreign servers.
    pub fhir_base_url: Option<String>,
}

/// Logical composition of trusted FHIR search predicates.
/// Each Query uses normal search semantics; alternatives represent the union
/// of grants, while conjunctions combine a grant's compartment and filters.
#[derive(Debug, Clone)]
pub enum SearchConstraint {
    /// Every child must match; an empty conjunction is true.
    And(Vec<SearchConstraint>),
    /// At least one child must match; an empty union denies all rows.
    Or(Vec<SearchConstraint>),
    /// Search criteria only. Unknown, empty or ignored predicates are rejected.
    Query(HashMap<String, Vec<String>>),
    /// FHIR Patient compartment membership, including incoming, transitive
    /// Patient.link edges. This is a search selection, never a SMART grant.
    LinkedPatient {
        patient_id: String,
        fhir_base_url: String,
        parameters: Vec<String>,
    },
    /// A grouped selection of Patient compartments. All roots share one
    /// recursive membership query; this does not confer SMART launch rights.
    LinkedPatients {
        patient_ids: Vec<String>,
        fhir_base_url: String,
        parameters: Vec<String>,
    },
    /// Select records belonging to any current Patient. This is an internal
    /// membership predicate, intersected with the caller's independent grants.
    AllPatients {
        fhir_base_url: String,
        parameters: Vec<String>,
    },
}

impl SearchParams {
    /// Creates new empty `SearchParams`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Select this type's trusted visibility, intersecting any global predicate.
    /// A missing entry in an aggregate visibility map denies this branch.
    #[must_use]
    pub fn for_resource_type(&self, resource_type: &str) -> Self {
        let mut params = self.clone();
        if let Some(constraints) = params.type_constraints.take() {
            let constraint = constraints
                .get(resource_type)
                .cloned()
                .unwrap_or_else(|| SearchConstraint::Or(Vec::new()));
            params.constraint = Some(match params.constraint.take() {
                Some(existing) => SearchConstraint::And(vec![existing, constraint]),
                None => constraint,
            });
        }
        params
    }

    /// Whether only the total is requested, without resources or pagination.
    #[must_use]
    pub fn is_count_only(&self) -> bool {
        self.count == Some(0)
            || self
                .parameters
                .get("_summary")
                .and_then(|values| values.first())
                .is_some_and(|value| value == "count")
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
        let result = SearchResult::empty().with_total(100).with_has_more(true);

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
