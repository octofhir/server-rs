//! App Manifest type definitions.
//!
//! These types define the structure of an App Manifest, which is a declarative
//! format for defining FHIR applications with operations, subscriptions,
//! and pre-created resources.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// App Manifest - a declarative format for defining FHIR applications.
///
/// # Example
///
/// ```json
/// {
///   "resourceType": "App",
///   "id": "my-app",
///   "name": "My Application",
///   "apiVersion": 1,
///   "secret": "my-app-secret-key",
///   "endpoint": {
///     "url": "http://backend:3000/api",
///     "timeout": 30
///   },
///   "operations": {
///     "my-operation": {
///       "method": "POST",
///       "path": ["my-app", "do-something"],
///       "policy": { "roles": ["admin"] }
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    /// Resource type (always "App")
    pub resource_type: String,

    /// Unique identifier for the app
    pub id: String,

    /// Human-readable name of the app
    pub name: String,

    /// API version for the manifest format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<u32>,

    /// Current status of the app
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AppStatus>,

    /// App secret for authentication (required, write-only)
    pub secret: String,

    /// Endpoint configuration for proxying requests
    pub endpoint: AppEndpoint,

    /// Operations (auto-creates CustomOperations)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub operations: HashMap<String, OperationDef>,

    /// Resources to pre-create (Client, AccessPolicy, etc.)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub resources: HashMap<String, HashMap<String, Value>>,

    /// Subscriptions for events/notifications
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub subscriptions: HashMap<String, SubscriptionDef>,
}

/// App status indicating whether the app is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppStatus {
    #[default]
    Active,
    Inactive,
    Suspended,
}

impl std::fmt::Display for AppStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppStatus::Active => write!(f, "active"),
            AppStatus::Inactive => write!(f, "inactive"),
            AppStatus::Suspended => write!(f, "suspended"),
        }
    }
}

/// Endpoint configuration for proxying requests to the app backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEndpoint {
    /// Target URL for proxying requests
    pub url: String,

    /// Request timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

/// Operation definition for custom API endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDef {
    /// HTTP method for this operation
    pub method: HttpMethod,

    /// Path segments for the operation URL
    pub path: Vec<PathSegment>,

    /// Whether this operation is public (no authentication required)
    #[serde(default)]
    pub public: bool,

    /// Policy configuration for authorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<OperationPolicy>,
}

/// Path segment - either a static string or a parameter.
///
/// # Examples
///
/// Static path: `["api", "users"]` -> `/api/users`
/// With param: `["api", {"name": "id"}]` -> `/api/:id`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSegment {
    /// Static path segment (e.g., "users")
    Static(String),
    /// Dynamic parameter (e.g., `:id`)
    Param {
        /// Parameter name
        name: String,
    },
}

impl PathSegment {
    /// Create a static path segment.
    pub fn static_segment(s: impl Into<String>) -> Self {
        PathSegment::Static(s.into())
    }

    /// Create a parameter path segment.
    pub fn param(name: impl Into<String>) -> Self {
        PathSegment::Param { name: name.into() }
    }

    /// Convert to URL path component.
    pub fn to_path_component(&self) -> String {
        match self {
            PathSegment::Static(s) => s.clone(),
            PathSegment::Param { name } => format!(":{name}"),
        }
    }
}

/// HTTP method for operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Delete => write!(f, "DELETE"),
        }
    }
}

/// Authentication type for operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AuthType {
    /// Forward OAuth token authentication (default)
    #[default]
    Forward,
    /// App secret authentication via X-App-Secret header
    App,
}


/// Policy configuration for operation authorization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPolicy {
    /// Authentication type: "forward" (OAuth token) or "app" (X-App-Secret)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<AuthType>,

    /// Required roles for access
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,

    /// Required OAuth scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,

    /// Whether authentication is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_auth: Option<bool>,

    /// Whether a FHIR user reference is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_fhir_user: Option<bool>,

    /// Compartment restriction ("patient" | "practitioner")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compartment: Option<String>,

    /// QuickJS script for custom policy logic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

/// Subscription definition for events and notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDef {
    /// Event trigger configuration
    pub trigger: SubscriptionTrigger,

    /// Channel for delivering events (webhook)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<SubscriptionChannel>,

    /// Notification configuration (for push notifications)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationDef>,
}

/// Subscription trigger configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionTrigger {
    /// FHIR resource type to watch
    pub resource_type: String,

    /// Event type (create, update, delete)
    pub event: SubscriptionEvent,

    /// FHIRPath filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhirpath: Option<String>,
}

/// Subscription event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionEvent {
    Create,
    Update,
    Delete,
}

impl std::fmt::Display for SubscriptionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscriptionEvent::Create => write!(f, "create"),
            SubscriptionEvent::Update => write!(f, "update"),
            SubscriptionEvent::Delete => write!(f, "delete"),
        }
    }
}

/// Subscription channel for webhook delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionChannel {
    /// Channel type (e.g., "webhook")
    #[serde(rename = "type")]
    pub channel_type: String,

    /// Target endpoint URL
    pub endpoint: String,
}

/// Notification definition for push notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDef {
    /// Reference to NotificationProvider resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Notification channel(s) (email, sms, push)
    pub channel: NotificationChannel,

    /// Template identifier or content
    pub template: String,

    /// Recipient configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<RecipientDef>,

    /// Delay configuration for scheduled notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<DelayDef>,
}

/// Notification channel - single or multiple channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotificationChannel {
    /// Single channel (e.g., "email")
    Single(String),
    /// Multiple channels (e.g., ["email", "sms"])
    Multiple(Vec<String>),
}

impl NotificationChannel {
    /// Get all channels as a vector.
    pub fn channels(&self) -> Vec<&str> {
        match self {
            NotificationChannel::Single(s) => vec![s.as_str()],
            NotificationChannel::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Recipient definition using FHIRPath.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientDef {
    /// FHIRPath expression to resolve recipient
    pub fhirpath: String,
}

/// Delay configuration for scheduled notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayDef {
    /// Field to calculate delay relative to (e.g., "start")
    pub relative_to: String,

    /// ISO 8601 duration offset (e.g., "-PT24H" for 24 hours before)
    pub offset: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_app_manifest() {
        let json = r#"{
            "resourceType": "App",
            "id": "psychportal",
            "name": "PsychPortal",
            "apiVersion": 1,
            "secret": "psychportal-secret-key",
            "endpoint": {
                "url": "http://backend:3000/api/operations",
                "timeout": 30
            },
            "operations": {
                "book-appointment": {
                    "method": "POST",
                    "path": ["psychportal", "appointments", "book"],
                    "policy": {
                        "roles": ["client"],
                        "compartment": "patient"
                    }
                }
            }
        }"#;

        let manifest: AppManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.id, "psychportal");
        assert_eq!(manifest.name, "PsychPortal");
        assert_eq!(manifest.api_version, Some(1));
        assert_eq!(manifest.secret, "psychportal-secret-key");
        assert_eq!(manifest.endpoint.url, "http://backend:3000/api/operations");
        assert_eq!(manifest.endpoint.timeout, Some(30));
        assert_eq!(manifest.operations.len(), 1);

        let op = manifest.operations.get("book-appointment").unwrap();
        assert_eq!(op.method, HttpMethod::Post);
        assert_eq!(op.path.len(), 3);
        assert!(!op.public);

        let policy = op.policy.as_ref().unwrap();
        assert_eq!(policy.roles, Some(vec!["client".to_string()]));
        assert_eq!(policy.compartment, Some("patient".to_string()));
    }

    #[test]
    fn test_parse_path_with_params() {
        let json = r#"{
            "method": "GET",
            "path": ["users", {"name": "id"}, "profile"],
            "public": true
        }"#;

        let op: OperationDef = serde_json::from_str(json).unwrap();
        assert_eq!(op.method, HttpMethod::Get);
        assert_eq!(op.path.len(), 3);
        assert!(op.public);

        // Check path segments
        match &op.path[0] {
            PathSegment::Static(s) => assert_eq!(s, "users"),
            _ => panic!("Expected static segment"),
        }
        match &op.path[1] {
            PathSegment::Param { name } => assert_eq!(name, "id"),
            _ => panic!("Expected param segment"),
        }
        match &op.path[2] {
            PathSegment::Static(s) => assert_eq!(s, "profile"),
            _ => panic!("Expected static segment"),
        }
    }

    #[test]
    fn test_parse_subscription() {
        let json = r#"{
            "trigger": {
                "resourceType": "Appointment",
                "event": "create",
                "fhirpath": "status = 'booked'"
            },
            "channel": {
                "type": "webhook",
                "endpoint": "http://backend:3000/webhooks/appointment"
            }
        }"#;

        let sub: SubscriptionDef = serde_json::from_str(json).unwrap();
        assert_eq!(sub.trigger.resource_type, "Appointment");
        assert_eq!(sub.trigger.event, SubscriptionEvent::Create);
        assert_eq!(sub.trigger.fhirpath, Some("status = 'booked'".to_string()));

        let channel = sub.channel.unwrap();
        assert_eq!(channel.channel_type, "webhook");
        assert_eq!(
            channel.endpoint,
            "http://backend:3000/webhooks/appointment"
        );
    }

    #[test]
    fn test_parse_notification() {
        let json = r#"{
            "trigger": {
                "resourceType": "Appointment",
                "event": "create"
            },
            "notification": {
                "provider": "NotificationProvider/twilio",
                "channel": ["email", "sms"],
                "template": "appointment-confirmation",
                "recipient": {
                    "fhirpath": "participant.actor.where(resolve().resourceType = 'Patient')"
                },
                "delay": {
                    "relativeTo": "start",
                    "offset": "-PT24H"
                }
            }
        }"#;

        let sub: SubscriptionDef = serde_json::from_str(json).unwrap();
        let notif = sub.notification.unwrap();

        assert_eq!(notif.provider, Some("NotificationProvider/twilio".to_string()));
        assert_eq!(notif.template, "appointment-confirmation");

        match &notif.channel {
            NotificationChannel::Multiple(channels) => {
                assert_eq!(channels, &vec!["email".to_string(), "sms".to_string()]);
            }
            _ => panic!("Expected multiple channels"),
        }

        let recipient = notif.recipient.unwrap();
        assert_eq!(
            recipient.fhirpath,
            "participant.actor.where(resolve().resourceType = 'Patient')"
        );

        let delay = notif.delay.unwrap();
        assert_eq!(delay.relative_to, "start");
        assert_eq!(delay.offset, "-PT24H");
    }

    #[test]
    fn test_notification_channel_single() {
        let json = r#""email""#;
        let channel: NotificationChannel = serde_json::from_str(json).unwrap();
        assert_eq!(channel.channels(), vec!["email"]);
    }

    #[test]
    fn test_app_status_default() {
        let status = AppStatus::default();
        assert_eq!(status, AppStatus::Active);
        assert_eq!(status.to_string(), "active");
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Put.to_string(), "PUT");
        assert_eq!(HttpMethod::Patch.to_string(), "PATCH");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_path_segment_helpers() {
        let static_seg = PathSegment::static_segment("users");
        assert_eq!(static_seg.to_path_component(), "users");

        let param_seg = PathSegment::param("id");
        assert_eq!(param_seg.to_path_component(), ":id");
    }

    #[test]
    fn test_serialize_app_manifest() {
        let manifest = AppManifest {
            resource_type: "App".to_string(),
            id: "test-app".to_string(),
            name: "Test App".to_string(),
            api_version: Some(1),
            status: Some(AppStatus::Active),
            secret: "test-secret-key".to_string(),
            endpoint: AppEndpoint {
                url: "http://localhost:3000".to_string(),
                timeout: Some(30),
            },
            operations: HashMap::new(),
            resources: HashMap::new(),
            subscriptions: HashMap::new(),
        };

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(json.contains("\"resourceType\": \"App\""));
        assert!(json.contains("\"id\": \"test-app\""));
        assert!(json.contains("\"status\": \"active\""));
        assert!(json.contains("\"secret\": \"test-secret-key\""));

        // Empty collections should be omitted
        assert!(!json.contains("operations"));
        assert!(!json.contains("resources"));
        assert!(!json.contains("subscriptions"));
    }

    #[test]
    fn test_minimal_manifest() {
        let json = r#"{
            "resourceType": "App",
            "id": "minimal",
            "name": "Minimal App",
            "secret": "minimal-secret",
            "endpoint": {
                "url": "http://localhost:3000"
            }
        }"#;

        let manifest: AppManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.id, "minimal");
        assert_eq!(manifest.secret, "minimal-secret");
        assert_eq!(manifest.api_version, None);
        assert_eq!(manifest.status, None);
        assert!(manifest.operations.is_empty());
        assert!(manifest.resources.is_empty());
        assert!(manifest.subscriptions.is_empty());
    }
}
