//! Admin API types.
//!
//! This module provides request/response types for admin endpoints.

use serde::{Deserialize, Serialize};

// =============================================================================
// Search Parameters
// =============================================================================

/// Search parameters for IdentityProvider queries.
#[derive(Debug, Default, Deserialize)]
pub struct IdpSearchParams {
    /// Filter by active status.
    pub active: Option<bool>,

    /// Filter by name (partial match).
    pub name: Option<String>,

    /// Maximum number of results to return.
    #[serde(rename = "_count")]
    pub count: Option<i64>,

    /// Number of results to skip.
    #[serde(rename = "_offset")]
    pub offset: Option<i64>,
}

/// Search parameters for User queries.
#[derive(Debug, Default, Deserialize)]
pub struct UserSearchParams {
    /// Filter by email address.
    pub email: Option<String>,

    /// Filter by username (partial match).
    pub username: Option<String>,

    /// Filter by active status.
    pub active: Option<bool>,

    /// Filter by linked identity provider ID.
    #[serde(rename = "identity-provider")]
    pub identity_provider: Option<String>,

    /// Maximum number of results to return.
    #[serde(rename = "_count")]
    pub count: Option<i64>,

    /// Number of results to skip.
    #[serde(rename = "_offset")]
    pub offset: Option<i64>,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request body for linking an external identity to a user.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkIdentityRequest {
    /// Identity provider ID.
    pub provider_id: String,

    /// External subject identifier from the identity provider.
    pub external_id: String,

    /// Email address from the identity provider (optional).
    pub email: Option<String>,
}

/// Request body for unlinking an external identity from a user.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkIdentityRequest {
    /// Identity provider ID to unlink.
    pub provider_id: String,
}

// =============================================================================
// Response Types
// =============================================================================

/// FHIR Bundle resource for search results.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle<T> {
    /// Always "Bundle".
    pub resource_type: &'static str,

    /// Bundle type (searchset for search results).
    #[serde(rename = "type")]
    pub type_: &'static str,

    /// Total number of matching resources (before pagination).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,

    /// Bundle entries containing the resources.
    #[serde(default)]
    pub entry: Vec<BundleEntry<T>>,
}

impl<T> Bundle<T> {
    /// Creates a new search result bundle.
    pub fn searchset(resources: Vec<T>, total: Option<usize>) -> Self {
        Self {
            resource_type: "Bundle",
            type_: "searchset",
            total,
            entry: resources.into_iter().map(BundleEntry::new).collect(),
        }
    }
}

impl<T> Default for Bundle<T> {
    fn default() -> Self {
        Self {
            resource_type: "Bundle",
            type_: "searchset",
            total: None,
            entry: Vec::new(),
        }
    }
}

/// Bundle entry containing a single resource.
#[derive(Debug, Serialize)]
pub struct BundleEntry<T> {
    /// The resource in this entry.
    pub resource: T,
}

impl<T> BundleEntry<T> {
    /// Creates a new bundle entry.
    pub fn new(resource: T) -> Self {
        Self { resource }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_searchset() {
        let resources = vec!["a", "b", "c"];
        let bundle = Bundle::searchset(resources, Some(10));

        assert_eq!(bundle.resource_type, "Bundle");
        assert_eq!(bundle.type_, "searchset");
        assert_eq!(bundle.total, Some(10));
        assert_eq!(bundle.entry.len(), 3);
    }

    #[test]
    fn test_bundle_serialization() {
        let bundle: Bundle<serde_json::Value> = Bundle::searchset(vec![], Some(0));
        let json = serde_json::to_string(&bundle).unwrap();

        assert!(json.contains(r#""resourceType":"Bundle""#));
        assert!(json.contains(r#""type":"searchset""#));
    }

    #[test]
    fn test_idp_search_params_defaults() {
        let params: IdpSearchParams = serde_json::from_str("{}").unwrap();

        assert!(params.active.is_none());
        assert!(params.name.is_none());
        assert!(params.count.is_none());
        assert!(params.offset.is_none());
    }

    #[test]
    fn test_link_identity_request_deserialization() {
        let json = r#"{
            "providerId": "google",
            "externalId": "abc123",
            "email": "user@gmail.com"
        }"#;

        let request: LinkIdentityRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.provider_id, "google");
        assert_eq!(request.external_id, "abc123");
        assert_eq!(request.email, Some("user@gmail.com".to_string()));
    }
}
