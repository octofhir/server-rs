//! Event types for the unified resource event system.
//!
//! This module defines the core event types used for inter-module communication:
//! - `ResourceEvent` - FHIR resource CRUD operations
//! - `AuthEvent` - Authentication and session events
//! - `SystemEvent` - Unified enum combining all event types

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use time::OffsetDateTime;

// ============================================================================
// Resource Events
// ============================================================================

/// Type of resource change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceEventType {
    /// Resource was created
    Created,
    /// Resource was updated
    Updated,
    /// Resource was deleted
    Deleted,
}

impl ResourceEventType {
    /// Returns the string representation of the event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceEventType::Created => "created",
            ResourceEventType::Updated => "updated",
            ResourceEventType::Deleted => "deleted",
        }
    }
}

impl std::fmt::Display for ResourceEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Event representing a change to a FHIR resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEvent {
    /// Type of change (created, updated, deleted)
    pub event_type: ResourceEventType,
    /// FHIR resource type (e.g., "Patient", "Observation", "AccessPolicy")
    pub resource_type: String,
    /// Resource ID
    pub resource_id: String,
    /// Version ID (transaction ID) if available
    pub version_id: Option<i64>,
    /// The resource data as JSON (None for deletions)
    pub resource: Option<serde_json::Value>,
    /// Timestamp of the event
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
}

impl ResourceEvent {
    /// Create a new resource event.
    pub fn new(
        event_type: ResourceEventType,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: Option<serde_json::Value>,
    ) -> Self {
        Self {
            event_type,
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            version_id: None,
            resource,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Create a "created" event.
    pub fn created(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> Self {
        Self::new(
            ResourceEventType::Created,
            resource_type,
            resource_id,
            Some(resource),
        )
    }

    /// Create an "updated" event.
    pub fn updated(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> Self {
        Self::new(
            ResourceEventType::Updated,
            resource_type,
            resource_id,
            Some(resource),
        )
    }

    /// Create a "deleted" event.
    pub fn deleted(resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        Self::new(ResourceEventType::Deleted, resource_type, resource_id, None)
    }

    /// Set the version ID.
    pub fn with_version(mut self, version_id: i64) -> Self {
        self.version_id = Some(version_id);
        self
    }

    /// Check if this event matches a filter by resource type.
    pub fn matches_type(&self, filter_type: Option<&str>) -> bool {
        match filter_type {
            Some(t) => self.resource_type == t,
            None => true, // No filter means match all
        }
    }

    /// Check if this event matches a filter by event type.
    pub fn matches_event_type(&self, filter: Option<ResourceEventType>) -> bool {
        match filter {
            Some(t) => self.event_type == t,
            None => true, // No filter means match all
        }
    }
}

// ============================================================================
// Auth Events
// ============================================================================

/// Type of authentication event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthEventType {
    /// User session was created (login)
    SessionCreated,
    /// User session was revoked (logout)
    SessionRevoked,
    /// Login succeeded
    LoginSucceeded,
    /// Login failed (wrong password, locked account, etc.)
    LoginFailed,
    /// Access token was refreshed
    TokenRefreshed,
    /// Token was revoked
    TokenRevoked,
}

impl AuthEventType {
    /// Returns the string representation of the event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthEventType::SessionCreated => "session_created",
            AuthEventType::SessionRevoked => "session_revoked",
            AuthEventType::LoginSucceeded => "login_succeeded",
            AuthEventType::LoginFailed => "login_failed",
            AuthEventType::TokenRefreshed => "token_refreshed",
            AuthEventType::TokenRevoked => "token_revoked",
        }
    }
}

impl std::fmt::Display for AuthEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Event representing an authentication-related action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    /// Type of auth event
    pub event_type: AuthEventType,
    /// User ID (if known)
    pub user_id: Option<String>,
    /// Client ID
    pub client_id: String,
    /// Session ID (if applicable)
    pub session_id: Option<String>,
    /// Token JTI (if applicable)
    pub token_jti: Option<String>,
    /// Source IP address
    #[serde(
        serialize_with = "serialize_ip_option",
        deserialize_with = "deserialize_ip_option",
        default
    )]
    pub ip_address: Option<IpAddr>,
    /// Reason for the event (e.g., failure reason)
    pub reason: Option<String>,
    /// Timestamp of the event
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
}

fn serialize_ip_option<S>(ip: &Option<IpAddr>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match ip {
        Some(addr) => serializer.serialize_some(&addr.to_string()),
        None => serializer.serialize_none(),
    }
}

fn deserialize_ip_option<'de, D>(deserializer: D) -> Result<Option<IpAddr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

impl AuthEvent {
    /// Create a new auth event.
    pub fn new(event_type: AuthEventType, client_id: impl Into<String>) -> Self {
        Self {
            event_type,
            user_id: None,
            client_id: client_id.into(),
            session_id: None,
            token_jti: None,
            ip_address: None,
            reason: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Create a login succeeded event.
    pub fn login_succeeded(
        user_id: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            event_type: AuthEventType::LoginSucceeded,
            user_id: Some(user_id.into()),
            client_id: client_id.into(),
            session_id: None,
            token_jti: None,
            ip_address: None,
            reason: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Create a login failed event.
    pub fn login_failed(
        client_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            event_type: AuthEventType::LoginFailed,
            user_id: None,
            client_id: client_id.into(),
            session_id: None,
            token_jti: None,
            ip_address: None,
            reason: Some(reason.into()),
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Create a session created event.
    pub fn session_created(
        user_id: impl Into<String>,
        client_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            event_type: AuthEventType::SessionCreated,
            user_id: Some(user_id.into()),
            client_id: client_id.into(),
            session_id: Some(session_id.into()),
            token_jti: None,
            ip_address: None,
            reason: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Create a token revoked event.
    pub fn token_revoked(
        client_id: impl Into<String>,
        token_jti: impl Into<String>,
    ) -> Self {
        Self {
            event_type: AuthEventType::TokenRevoked,
            user_id: None,
            client_id: client_id.into(),
            session_id: None,
            token_jti: Some(token_jti.into()),
            ip_address: None,
            reason: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }

    /// Set the user ID.
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set the session ID.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set the IP address.
    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip_address = Some(ip);
        self
    }

    /// Set the reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

// ============================================================================
// System Event (Unified)
// ============================================================================

/// Unified event enum combining all event types.
///
/// This is the main event type that flows through the event system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemEvent {
    /// A FHIR resource was created, updated, or deleted
    Resource(ResourceEvent),
    /// An authentication event occurred
    Auth(AuthEvent),
}

impl SystemEvent {
    /// Create a resource event.
    pub fn resource(event: ResourceEvent) -> Self {
        SystemEvent::Resource(event)
    }

    /// Create an auth event.
    pub fn auth(event: AuthEvent) -> Self {
        SystemEvent::Auth(event)
    }

    /// Get the timestamp of the event.
    pub fn timestamp(&self) -> OffsetDateTime {
        match self {
            SystemEvent::Resource(e) => e.timestamp,
            SystemEvent::Auth(e) => e.timestamp,
        }
    }

    /// Get the resource event if this is a resource event.
    pub fn as_resource(&self) -> Option<&ResourceEvent> {
        match self {
            SystemEvent::Resource(e) => Some(e),
            _ => None,
        }
    }

    /// Get the auth event if this is an auth event.
    pub fn as_auth(&self) -> Option<&AuthEvent> {
        match self {
            SystemEvent::Auth(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ResourceEvent> for SystemEvent {
    fn from(event: ResourceEvent) -> Self {
        SystemEvent::Resource(event)
    }
}

impl From<AuthEvent> for SystemEvent {
    fn from(event: AuthEvent) -> Self {
        SystemEvent::Auth(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_event_created() {
        let event = ResourceEvent::created("Patient", "123", serde_json::json!({"id": "123"}));
        assert_eq!(event.event_type, ResourceEventType::Created);
        assert_eq!(event.resource_type, "Patient");
        assert_eq!(event.resource_id, "123");
        assert!(event.resource.is_some());
    }

    #[test]
    fn test_resource_event_matches() {
        let event = ResourceEvent::created("Patient", "123", serde_json::json!({}));
        assert!(event.matches_type(Some("Patient")));
        assert!(!event.matches_type(Some("Observation")));
        assert!(event.matches_type(None));
        assert!(event.matches_event_type(Some(ResourceEventType::Created)));
        assert!(!event.matches_event_type(Some(ResourceEventType::Deleted)));
    }

    #[test]
    fn test_auth_event_login() {
        let event = AuthEvent::login_succeeded("user-1", "client-1");
        assert_eq!(event.event_type, AuthEventType::LoginSucceeded);
        assert_eq!(event.user_id, Some("user-1".to_string()));
        assert_eq!(event.client_id, "client-1");
    }

    #[test]
    fn test_system_event_from() {
        let resource_event = ResourceEvent::created("Patient", "123", serde_json::json!({}));
        let system_event: SystemEvent = resource_event.into();
        assert!(system_event.as_resource().is_some());
        assert!(system_event.as_auth().is_none());
    }

    #[test]
    fn test_event_serialization() {
        let event = ResourceEvent::created("Patient", "123", serde_json::json!({"id": "123"}));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ResourceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resource_type, "Patient");
        assert_eq!(parsed.resource_id, "123");
    }
}
