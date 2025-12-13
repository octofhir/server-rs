//! Reference DataLoader for parsing and resolving FHIR references.
//!
//! This loader parses FHIR reference strings and resolves them to actual
//! resources using the ResourceLoader for efficient batching.

use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::dataloader::Loader;
use octofhir_storage::DynStorage;
use tracing::{debug, instrument, trace, warn};

use crate::error::GraphQLError;

/// A parsed FHIR reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReference {
    /// The resource type (e.g., "Patient"). Empty for contained references.
    pub resource_type: String,
    /// The resource ID.
    pub id: String,
    /// Whether this is an absolute URL reference.
    pub is_absolute: bool,
    /// Whether this is a contained reference (starts with #).
    pub is_contained: bool,
    /// The original reference string.
    pub original: String,
}

impl ParsedReference {
    /// Parses a FHIR reference string.
    ///
    /// Supports:
    /// - Relative references: `Patient/123`
    /// - Absolute URLs: `http://example.org/fhir/Patient/123`
    /// - Contained references: `#contained-id`
    ///
    /// Returns `None` if the reference cannot be parsed.
    #[must_use]
    pub fn parse(reference: &str) -> Option<Self> {
        let reference = reference.trim();

        if reference.is_empty() {
            return None;
        }

        // Handle contained references
        if let Some(contained_id) = reference.strip_prefix('#') {
            if contained_id.is_empty() {
                return None;
            }
            return Some(Self {
                resource_type: String::new(),
                id: contained_id.to_string(),
                is_absolute: false,
                is_contained: true,
                original: reference.to_string(),
            });
        }

        // Check if it's an absolute URL
        let is_absolute = reference.starts_with("http://") || reference.starts_with("https://");

        // Split by '/' and take last two parts (Type/id)
        let parts: Vec<&str> = reference.split('/').collect();
        if parts.len() < 2 {
            return None;
        }

        let type_index = parts.len() - 2;
        let id_index = parts.len() - 1;

        let resource_type = parts[type_index];
        let id = parts[id_index];

        // Validate resource type (should start with uppercase letter)
        if resource_type.is_empty()
            || !resource_type.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
        {
            return None;
        }

        // Validate ID is not empty
        if id.is_empty() {
            return None;
        }

        Some(Self {
            resource_type: resource_type.to_string(),
            id: id.to_string(),
            is_absolute,
            is_contained: false,
            original: reference.to_string(),
        })
    }

    /// Returns the reference as a relative reference string.
    #[must_use]
    pub fn as_relative(&self) -> String {
        if self.is_contained {
            format!("#{}", self.id)
        } else {
            format!("{}/{}", self.resource_type, self.id)
        }
    }
}

/// Key for looking up a reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReferenceKey {
    /// The original reference string.
    pub reference: String,
}

impl ReferenceKey {
    /// Creates a new reference key.
    #[must_use]
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
        }
    }
}

impl std::fmt::Display for ReferenceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reference)
    }
}

/// Result of resolving a reference.
#[derive(Debug, Clone)]
pub struct ResolvedReference {
    /// The parsed reference information.
    pub parsed: ParsedReference,
    /// The resolved resource, if found.
    pub resource: Option<serde_json::Value>,
}

/// DataLoader for resolving FHIR references.
///
/// This loader parses reference strings, groups them by resource type,
/// and fetches resources in batches.
pub struct ReferenceLoader {
    storage: DynStorage,
}

impl ReferenceLoader {
    /// Creates a new reference loader.
    #[must_use]
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

impl Loader<ReferenceKey> for ReferenceLoader {
    type Value = ResolvedReference;
    type Error = Arc<GraphQLError>;

    #[instrument(skip(self, keys), fields(key_count = keys.len()))]
    async fn load(
        &self,
        keys: &[ReferenceKey],
    ) -> Result<HashMap<ReferenceKey, Self::Value>, Self::Error> {
        debug!(key_count = keys.len(), "Resolving references batch");

        let mut results: HashMap<ReferenceKey, ResolvedReference> = HashMap::with_capacity(keys.len());

        // Parse all references first
        let mut parsed_refs: Vec<(ReferenceKey, ParsedReference)> = Vec::with_capacity(keys.len());
        let mut contained_refs: Vec<(ReferenceKey, ParsedReference)> = Vec::new();

        for key in keys {
            match ParsedReference::parse(&key.reference) {
                Some(parsed) if parsed.is_contained => {
                    // Contained references need special handling
                    contained_refs.push((key.clone(), parsed));
                }
                Some(parsed) => {
                    parsed_refs.push((key.clone(), parsed));
                }
                None => {
                    warn!(reference = %key.reference, "Failed to parse reference");
                    // Still add to results with None resource
                    results.insert(
                        key.clone(),
                        ResolvedReference {
                            parsed: ParsedReference {
                                resource_type: String::new(),
                                id: String::new(),
                                is_absolute: false,
                                is_contained: false,
                                original: key.reference.clone(),
                            },
                            resource: None,
                        },
                    );
                }
            }
        }

        // Handle contained references (these need the parent resource context)
        // For now, we return them with None resource - the resolver needs to handle them
        for (key, parsed) in contained_refs {
            trace!(reference = %key.reference, "Contained reference - needs parent context");
            results.insert(
                key,
                ResolvedReference {
                    parsed,
                    resource: None,
                },
            );
        }

        // Group by resource type for batched loading
        let mut by_type: HashMap<&str, Vec<(&ReferenceKey, &ParsedReference)>> = HashMap::new();
        for (key, parsed) in &parsed_refs {
            by_type.entry(&parsed.resource_type).or_default().push((key, parsed));
        }

        // Fetch resources by type
        for (resource_type, refs) in by_type {
            trace!(
                resource_type = %resource_type,
                count = refs.len(),
                "Fetching references for type"
            );

            for (key, parsed) in refs {
                match self.storage.read(resource_type, &parsed.id).await {
                    Ok(Some(stored)) => {
                        results.insert(
                            (*key).clone(),
                            ResolvedReference {
                                parsed: parsed.clone(),
                                resource: Some(stored.resource),
                            },
                        );
                    }
                    Ok(None) => {
                        trace!(reference = %key.reference, "Referenced resource not found");
                        results.insert(
                            (*key).clone(),
                            ResolvedReference {
                                parsed: parsed.clone(),
                                resource: None,
                            },
                        );
                    }
                    Err(e) => {
                        warn!(
                            reference = %key.reference,
                            error = %e,
                            "Failed to load referenced resource"
                        );
                        results.insert(
                            (*key).clone(),
                            ResolvedReference {
                                parsed: parsed.clone(),
                                resource: None,
                            },
                        );
                    }
                }
            }
        }

        debug!(
            requested = keys.len(),
            resolved = results.iter().filter(|(_, v)| v.resource.is_some()).count(),
            "Reference resolution complete"
        );

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_reference() {
        let parsed = ParsedReference::parse("Patient/123").unwrap();
        assert_eq!(parsed.resource_type, "Patient");
        assert_eq!(parsed.id, "123");
        assert!(!parsed.is_absolute);
        assert!(!parsed.is_contained);
    }

    #[test]
    fn test_parse_absolute_reference() {
        let parsed = ParsedReference::parse("http://example.org/fhir/Patient/456").unwrap();
        assert_eq!(parsed.resource_type, "Patient");
        assert_eq!(parsed.id, "456");
        assert!(parsed.is_absolute);
        assert!(!parsed.is_contained);
    }

    #[test]
    fn test_parse_https_reference() {
        let parsed = ParsedReference::parse("https://example.org/fhir/Observation/obs-1").unwrap();
        assert_eq!(parsed.resource_type, "Observation");
        assert_eq!(parsed.id, "obs-1");
        assert!(parsed.is_absolute);
    }

    #[test]
    fn test_parse_contained_reference() {
        let parsed = ParsedReference::parse("#contained-med").unwrap();
        assert_eq!(parsed.id, "contained-med");
        assert!(parsed.is_contained);
        assert!(parsed.resource_type.is_empty());
    }

    #[test]
    fn test_parse_invalid_references() {
        assert!(ParsedReference::parse("").is_none());
        assert!(ParsedReference::parse("invalid").is_none());
        assert!(ParsedReference::parse("patient/123").is_none()); // lowercase
        assert!(ParsedReference::parse("/123").is_none());
        assert!(ParsedReference::parse("#").is_none()); // empty contained
    }

    #[test]
    fn test_as_relative() {
        let parsed = ParsedReference::parse("http://example.org/fhir/Patient/123").unwrap();
        assert_eq!(parsed.as_relative(), "Patient/123");

        let contained = ParsedReference::parse("#contained-id").unwrap();
        assert_eq!(contained.as_relative(), "#contained-id");
    }

    #[test]
    fn test_reference_key() {
        let key = ReferenceKey::new("Patient/123");
        assert_eq!(key.reference, "Patient/123");
        assert_eq!(format!("{}", key), "Patient/123");
    }
}
