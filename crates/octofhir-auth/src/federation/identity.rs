//! External identity types and helpers.
//!
//! This module provides types for linking external identity provider accounts
//! to local users.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::identity::{UserIdentity, get_identities, add_identity};
//! use octofhir_auth::storage::User;
//! use time::OffsetDateTime;
//!
//! let mut user = User::new("testuser");
//!
//! // Add an identity from Google
//! let identity = UserIdentity {
//!     provider_id: "google".to_string(),
//!     external_subject: "abc123".to_string(),
//!     email: Some("user@gmail.com".to_string()),
//!     linked_at: OffsetDateTime::now_utc(),
//! };
//! add_identity(&mut user, identity);
//!
//! // Get all identities
//! let identities = get_identities(&user);
//! assert_eq!(identities.len(), 1);
//! ```

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::User;

/// Key used to store identities in User.attributes.
pub const IDENTITIES_KEY: &str = "identities";

/// An external identity provider link.
///
/// Represents a connection between a local user account and an external
/// identity provider account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserIdentity {
    /// The identity provider ID (e.g., "google", "azure-ad").
    pub provider_id: String,

    /// The subject identifier from the IdP (unique per provider).
    pub external_subject: String,

    /// Email address from the IdP (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// When this identity was linked.
    #[serde(with = "time::serde::rfc3339")]
    pub linked_at: OffsetDateTime,
}

impl UserIdentity {
    /// Creates a new identity link.
    #[must_use]
    pub fn new(provider_id: impl Into<String>, external_subject: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            external_subject: external_subject.into(),
            email: None,
            linked_at: OffsetDateTime::now_utc(),
        }
    }

    /// Sets the email for this identity.
    #[must_use]
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Sets the linked_at timestamp.
    #[must_use]
    pub fn with_linked_at(mut self, linked_at: OffsetDateTime) -> Self {
        self.linked_at = linked_at;
        self
    }

    /// Checks if this identity matches the given provider and subject.
    #[must_use]
    pub fn matches(&self, provider_id: &str, external_subject: &str) -> bool {
        self.provider_id == provider_id && self.external_subject == external_subject
    }
}

/// Gets all external identities for a user.
///
/// Returns an empty vector if no identities are stored.
#[must_use]
pub fn get_identities(user: &User) -> Vec<UserIdentity> {
    user.attributes
        .get(IDENTITIES_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Sets the external identities for a user.
///
/// Replaces any existing identities.
pub fn set_identities(user: &mut User, identities: Vec<UserIdentity>) {
    if identities.is_empty() {
        user.attributes.remove(IDENTITIES_KEY);
    } else {
        user.attributes.insert(
            IDENTITIES_KEY.to_string(),
            serde_json::to_value(identities).expect("UserIdentity should serialize"),
        );
    }
}

/// Adds an external identity to a user.
///
/// If an identity with the same provider_id already exists, it is replaced.
pub fn add_identity(user: &mut User, identity: UserIdentity) {
    let mut identities = get_identities(user);

    // Remove existing identity for the same provider
    identities.retain(|i| i.provider_id != identity.provider_id);

    identities.push(identity);
    set_identities(user, identities);
}

/// Finds an identity by provider ID.
#[must_use]
pub fn find_identity_by_provider<'a>(
    identities: &'a [UserIdentity],
    provider_id: &str,
) -> Option<&'a UserIdentity> {
    identities.iter().find(|i| i.provider_id == provider_id)
}

/// Finds an identity by provider and subject.
#[must_use]
pub fn find_identity<'a>(
    identities: &'a [UserIdentity],
    provider_id: &str,
    external_subject: &str,
) -> Option<&'a UserIdentity> {
    identities
        .iter()
        .find(|i| i.matches(provider_id, external_subject))
}

/// Checks if a user has an identity from the given provider.
#[must_use]
pub fn has_identity_for_provider(user: &User, provider_id: &str) -> bool {
    get_identities(user)
        .iter()
        .any(|i| i.provider_id == provider_id)
}

/// Removes an identity from a user by provider ID.
///
/// Returns the removed identity if found.
pub fn remove_identity(user: &mut User, provider_id: &str) -> Option<UserIdentity> {
    let mut identities = get_identities(user);
    let index = identities
        .iter()
        .position(|i| i.provider_id == provider_id)?;
    let removed = identities.remove(index);
    set_identities(user, identities);
    Some(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_identity_new() {
        let identity = UserIdentity::new("google", "abc123");
        assert_eq!(identity.provider_id, "google");
        assert_eq!(identity.external_subject, "abc123");
        assert!(identity.email.is_none());
    }

    #[test]
    fn test_user_identity_builder() {
        let now = OffsetDateTime::now_utc();
        let identity = UserIdentity::new("azure-ad", "user-456")
            .with_email("user@example.com")
            .with_linked_at(now);

        assert_eq!(identity.provider_id, "azure-ad");
        assert_eq!(identity.external_subject, "user-456");
        assert_eq!(identity.email, Some("user@example.com".to_string()));
        assert_eq!(identity.linked_at, now);
    }

    #[test]
    fn test_user_identity_matches() {
        let identity = UserIdentity::new("google", "abc123");
        assert!(identity.matches("google", "abc123"));
        assert!(!identity.matches("google", "other"));
        assert!(!identity.matches("other", "abc123"));
    }

    #[test]
    fn test_get_identities_empty() {
        let user = User::new("testuser");
        let identities = get_identities(&user);
        assert!(identities.is_empty());
    }

    #[test]
    fn test_add_and_get_identities() {
        let mut user = User::new("testuser");

        let identity1 = UserIdentity::new("google", "g-123");
        let identity2 = UserIdentity::new("azure-ad", "az-456");

        add_identity(&mut user, identity1.clone());
        add_identity(&mut user, identity2.clone());

        let identities = get_identities(&user);
        assert_eq!(identities.len(), 2);
        assert!(find_identity(&identities, "google", "g-123").is_some());
        assert!(find_identity(&identities, "azure-ad", "az-456").is_some());
    }

    #[test]
    fn test_add_identity_replaces_same_provider() {
        let mut user = User::new("testuser");

        let identity1 = UserIdentity::new("google", "old-subject");
        let identity2 = UserIdentity::new("google", "new-subject");

        add_identity(&mut user, identity1);
        add_identity(&mut user, identity2);

        let identities = get_identities(&user);
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].external_subject, "new-subject");
    }

    #[test]
    fn test_set_identities() {
        let mut user = User::new("testuser");

        let identities = vec![
            UserIdentity::new("google", "g-123"),
            UserIdentity::new("github", "gh-456"),
        ];

        set_identities(&mut user, identities);

        let retrieved = get_identities(&user);
        assert_eq!(retrieved.len(), 2);
    }

    #[test]
    fn test_set_empty_identities_removes_key() {
        let mut user = User::new("testuser");

        add_identity(&mut user, UserIdentity::new("google", "g-123"));
        assert!(user.attributes.contains_key(IDENTITIES_KEY));

        set_identities(&mut user, vec![]);
        assert!(!user.attributes.contains_key(IDENTITIES_KEY));
    }

    #[test]
    fn test_find_identity_by_provider() {
        let identities = vec![
            UserIdentity::new("google", "g-123"),
            UserIdentity::new("azure-ad", "az-456"),
        ];

        let found = find_identity_by_provider(&identities, "google");
        assert!(found.is_some());
        assert_eq!(found.unwrap().external_subject, "g-123");

        let not_found = find_identity_by_provider(&identities, "github");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_has_identity_for_provider() {
        let mut user = User::new("testuser");
        add_identity(&mut user, UserIdentity::new("google", "g-123"));

        assert!(has_identity_for_provider(&user, "google"));
        assert!(!has_identity_for_provider(&user, "github"));
    }

    #[test]
    fn test_remove_identity() {
        let mut user = User::new("testuser");
        add_identity(&mut user, UserIdentity::new("google", "g-123"));
        add_identity(&mut user, UserIdentity::new("github", "gh-456"));

        let removed = remove_identity(&mut user, "google");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().provider_id, "google");

        let identities = get_identities(&user);
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].provider_id, "github");
    }

    #[test]
    fn test_remove_identity_not_found() {
        let mut user = User::new("testuser");
        let removed = remove_identity(&mut user, "nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_identity_serialization() {
        let identity = UserIdentity::new("google", "abc123").with_email("user@example.com");

        let json = serde_json::to_value(&identity).unwrap();
        assert_eq!(json["provider_id"], "google");
        assert_eq!(json["external_subject"], "abc123");
        assert_eq!(json["email"], "user@example.com");

        let deserialized: UserIdentity = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, identity);
    }
}
